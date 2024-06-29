use alloc::{string::String, sync::Arc};

use core::{mem::ManuallyDrop, ops::Deref};

use alloc::vec;
use axerrno::{AxError, AxResult};
use riscv::register::sstatus::{self, Sstatus};

use crate::stdio::{Stderr, Stdin, Stdout};
use axfs::api::{FileIO, OpenFlags};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use memory_addr::VirtAddr;
use spinlock::SpinNoIrq;

#[cfg(feature = "monolithic")]
use axhal::arch::{write_page_table_root0, TrapFrame};

use crate::api::load_app;
use crate::fd_manager::FdManager;
use crate::run_queue::RUN_QUEUE;
use crate::signal::SignalModule;
use crate::Mutex;
use crate::{schedule::add_wait_for_exit_queue, AxTask, AxTaskRef};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use axhal::KERNEL_PROCESS_ID;
use axmem::MemorySet;
pub use taskctx::TaskInner;

/// Map from task id to arc pointer of task
pub static TID2TASK: Mutex<BTreeMap<u64, AxTaskRef>> = Mutex::new(BTreeMap::new());

const FD_LIMIT_ORIGIN: usize = 1025;

extern "C" {
    fn start_signal_trampoline();
}

pub struct Task {
    /// 父task号
    pub parent: AtomicU64,
    /// 子task
    pub children: Mutex<Vec<u64>>,
    /// 文件描述符管理器
    pub fd_manager: FdManager,
    /// task状态
    pub is_zombie: AtomicBool,
    /// 地址空间
    pub memory_set: Mutex<Arc<Mutex<MemorySet>>>,

    /// 用户堆基址，任何时候堆顶都不能比这个值小，理论上讲是一个常量
    pub heap_bottom: AtomicU64,

    /// 当前用户堆的堆顶，不能小于基址，不能大于基址加堆的最大大小
    pub heap_top: AtomicU64,

    /// 信号处理模块
    /// 第一维代表TaskID，第二维代表对应的信号处理模块
    pub signal_modules: Mutex<BTreeMap<u64, SignalModule>>,

    /// 是否被vfork阻塞
    pub blocked_by_vfork: Mutex<bool>,

    /// 该进程可执行文件所在的路径
    pub file_path: Mutex<String>,

    ///inner
    pub inner: SpinNoIrq<TaskInner>,

    ///trapframe
    pub tf: SpinNoIrq<TrapFrame>,
}

impl Task {
    /// get the task id
    pub fn tid(&self) -> u64 {
        self.inner.lock().id().as_u64()
    }

    /// get the parent task id
    pub fn get_parent(&self) -> u64 {
        self.parent.load(Ordering::Acquire)
    }

    /// set the parent task id
    pub fn set_parent(&self, parent: u64) {
        self.parent.store(parent, Ordering::Release)
    }

    /// get the exit code of the task
    pub fn get_exit_code(&self) -> i32 {
        self.inner.lock().get_exit_code()
    }

    /// set the exit code of the task
    pub fn set_exit_code(&self, exit_code: i32) {
        self.inner.lock().set_exit_code(exit_code)
    }

    /// whether the task is a zombie task
    pub fn get_zombie(&self) -> bool {
        self.is_zombie.load(Ordering::Acquire)
    }

    /// set the task as a zombie task
    pub fn set_zombie(&self, status: bool) {
        self.is_zombie.store(status, Ordering::Release)
    }

    /// get the heap top of the task
    pub fn get_heap_top(&self) -> u64 {
        self.heap_top.load(Ordering::Acquire)
    }

    /// set the heap top of the task
    pub fn set_heap_top(&self, top: u64) {
        self.heap_top.store(top, Ordering::Release)
    }

    /// get the heap bottom of the task
    pub fn get_heap_bottom(&self) -> u64 {
        self.heap_bottom.load(Ordering::Acquire)
    }

    /// set the heap bottom of the task
    pub fn set_heap_bottom(&self, bottom: u64) {
        self.heap_bottom.store(bottom, Ordering::Release)
    }

    /// set the process as blocked by vfork
    pub fn set_vfork_block(&self, value: bool) {
        *self.blocked_by_vfork.lock() = value;
    }

    /// set the executable file path of the task
    pub fn set_file_path(&self, path: String) {
        let mut file_path = self.file_path.lock();
        *file_path = path;
    }

    /// get the executable file path of the task
    pub fn get_file_path(&self) -> String {
        (*self.file_path.lock()).clone()
    }

    /// 若进程运行完成，则获取其返回码
    /// 若正在运行（可能上锁或没有上锁），则返回None
    pub fn get_code_if_exit(&self) -> Option<i32> {
        if self.get_zombie() {
            return Some(self.get_exit_code());
        }
        None
    }

    pub fn get_entry(&self) -> Option<*mut dyn FnOnce()> {
        self.inner.lock().get_entry()
    }

    pub fn is_kernel_task(&self) -> bool {
        if self.inner.lock().name() == "gc"
            || self.inner.lock().name() == "idle"
            || self.inner.lock().name() == "main"
        {
            return true;
        }
        return false;
    }

    pub fn get_tf(&self) -> TrapFrame {
        self.tf.lock().clone()
    }

    pub fn set_tf(&self, tf: TrapFrame) {
        *self.tf.lock() = tf;
    }

    pub fn init_tf(&mut self, entry: usize, kstack_top: VirtAddr, tls: VirtAddr) {
        self.tf.lock().kernel_sp = kstack_top.as_usize();
        self.tf.lock().kernel_ra = entry;
        self.tf.lock().kernel_tp = tls.as_usize();
    }

    pub fn app_init_tf(&self, entry: usize, user_sp: usize) {
        let sstatus = sstatus::read();
        // 当前版本的riscv不支持使用set_spp函数，需要手动修改
        // 修改当前的sstatus为User，即是第8位置0
        self.tf.lock().set_user_sp(user_sp);
        self.tf.lock().set_pc(entry);
        let ss: usize =
            unsafe { (*(&sstatus as *const Sstatus as *const usize) & !(1 << 8)) & !(1 << 1) };
        self.tf.lock().set_ss(ss);
        let ta0: usize = unsafe { *(user_sp as *const usize) };
        let ta1: usize = unsafe { *(user_sp as *const usize).add(1) };
        self.tf.lock().set_arg0(ta0);
        self.tf.lock().set_arg1(ta1);
    }
}

impl Task {
    /// 创建一个新的task
    pub fn new(
        parent: u64,
        memory_set: Mutex<Arc<Mutex<MemorySet>>>,
        heap_bottom: u64,
        fd_table: Vec<Option<Arc<dyn FileIO>>>,
        inn: TaskInner,
    ) -> Self {
        inn.set_exit_code(0);
        Self {
            parent: AtomicU64::new(parent),
            children: Mutex::new(Vec::new()),
            is_zombie: AtomicBool::new(false),
            memory_set,
            heap_bottom: AtomicU64::new(heap_bottom),
            heap_top: AtomicU64::new(heap_bottom),
            fd_manager: FdManager::new(fd_table, FD_LIMIT_ORIGIN),
            signal_modules: Mutex::new(BTreeMap::new()),
            blocked_by_vfork: Mutex::new(false),
            file_path: Mutex::new(String::new()),
            inner: SpinNoIrq::new(inn),
            tf: SpinNoIrq::new(TrapFrame::default()),
        }
    }
    /// 根据给定参数创建一个新的进程，作为应用程序初始进程
    pub fn init(args: Vec<String>, envs: &Vec<String>) -> AxResult<AxTaskRef> {
        let path = args[0].clone();
        let mut memory_set = MemorySet::new_memory_set();
        {
            use axhal::mem::virt_to_phys;
            use axhal::paging::MappingFlags;
            // 生成信号跳板
            let signal_trampoline_vaddr: VirtAddr = (axconfig::SIGNAL_TRAMPOLINE).into();
            let signal_trampoline_paddr = virt_to_phys((start_signal_trampoline as usize).into());
            memory_set.map_page_without_alloc(
                signal_trampoline_vaddr,
                signal_trampoline_paddr,
                MappingFlags::READ
                    | MappingFlags::EXECUTE
                    | MappingFlags::USER
                    | MappingFlags::WRITE,
            )?;
        }
        let page_table_token = memory_set.page_table_token();
        if page_table_token != 0 {
            unsafe {
                write_page_table_root0(page_table_token.into());
                #[cfg(target_arch = "riscv64")]
                riscv::register::sstatus::set_sum();
            };
        }
        let (entry, user_stack_bottom, heap_bottom) =
            if let Ok(ans) = load_app(path.clone(), args, envs, &mut memory_set) {
                ans
            } else {
                error!("Failed to load app {}", path);
                return Err(AxError::NotFound);
            };
        let new_taski = new_task_inner(
            || {},
            path,
            axconfig::TASK_STACK_SIZE,
            page_table_token,
            false,
        );
        let mut new_task = Self::new(
            KERNEL_PROCESS_ID,
            Mutex::new(Arc::new(Mutex::new(memory_set))),
            heap_bottom.as_usize() as u64,
            vec![
                // 标准输入
                Some(Arc::new(Stdin {
                    flags: Mutex::new(OpenFlags::empty()),
                })),
                // 标准输出
                Some(Arc::new(Stdout {
                    flags: Mutex::new(OpenFlags::empty()),
                })),
                // 标准错误
                Some(Arc::new(Stderr {
                    flags: Mutex::new(OpenFlags::empty()),
                })),
            ],
            new_taski,
        );
        #[cfg(feature = "tls")]
        let tls = VirtAddr::from(task.get_tls_ptr());
        #[cfg(not(feature = "tls"))]
        let tls = VirtAddr::from(0);
        new_task.init_tf(
            task_entry as usize,
            (RUN_QUEUE.lock().get_kernel_stack_top() - core::mem::size_of::<TrapFrame>()).into(),
            tls,
        );
        new_task.app_init_tf(entry.as_usize(), user_stack_bottom.as_usize());
        // 将其作为内核进程的子进程
        match TID2TASK.lock().get(&KERNEL_PROCESS_ID) {
            Some(kernel_task) => {
                kernel_task.children.lock().push(new_task.tid());
            }
            None => {
                return Err(AxError::NotFound);
            }
        }
        let axtask = Arc::new(AxTask::new(new_task));
        TID2TASK.lock().insert(axtask.tid(), axtask.clone());
        RUN_QUEUE.lock().taskadd();
        RUN_QUEUE.lock().add_task(Arc::clone(&axtask.clone()));
        Ok(axtask.clone())
    }
}

/// 与地址空间相关的进程方法
impl Task {
    /// alloc physical memory for lazy allocation manually
    pub fn manual_alloc_for_lazy(&self, addr: VirtAddr) -> AxResult<()> {
        self.memory_set.lock().lock().manual_alloc_for_lazy(addr)
    }

    /// alloc range physical memory for lazy allocation manually
    pub fn manual_alloc_range_for_lazy(&self, start: VirtAddr, end: VirtAddr) -> AxResult<()> {
        self.memory_set
            .lock()
            .lock()
            .manual_alloc_range_for_lazy(start, end)
    }

    /// alloc physical memory with the given type size for lazy allocation manually
    pub fn manual_alloc_type_for_lazy<T: Sized>(&self, obj: *const T) -> AxResult<()> {
        self.memory_set
            .lock()
            .lock()
            .manual_alloc_type_for_lazy(obj)
    }
}

/// 与文件相关的进程方法
impl Task {
    /// 为进程分配一个文件描述符
    pub fn alloc_fd(&self, fd_table: &mut Vec<Option<Arc<dyn FileIO>>>) -> AxResult<usize> {
        for (i, fd) in fd_table.iter().enumerate() {
            if fd.is_none() {
                return Ok(i);
            }
        }
        if fd_table.len() >= self.fd_manager.get_limit() as usize {
            debug!("fd table is full");
            return Err(AxError::StorageFull);
        }
        fd_table.push(None);
        Ok(fd_table.len() - 1)
    }

    /// 获取当前进程的工作目录
    pub fn get_cwd(&self) -> String {
        self.fd_manager.cwd.lock().clone()
    }
}

extern "C" {
    fn _stdata();
    fn _etdata();
    fn _etbss();
}

#[cfg(feature = "tls")]
pub(crate) fn tls_area() -> (usize, usize) {
    (_stdata as usize, _etbss as usize)
}

#[cfg(feature = "monolithic")]
/// Create a new task.
///
/// # Arguments
/// - `entry`: The entry function of the task.
/// - `name`: The name of the task.
/// - `stack_size`: The size of the stack.
/// - `process_id`: The process ID of the task.
/// - `page_table_token`: The page table token of the task.
/// - `sig_child`: Whether the task will send a signal to its parent when it exits.
pub fn new_task_inner<F>(
    entry: F,
    name: String,
    stack_size: usize,
    page_table_token: usize,
    sig_child: bool,
) -> TaskInner
where
    F: FnOnce() + Send + 'static,
{
    use axhal::time::current_time_nanos;

    let task = taskctx::TaskInner::new(
        entry,
        name,
        stack_size,
        page_table_token,
        sig_child,
        #[cfg(feature = "tls")]
        tls_area(),
    );

    // 设置 CPU 亲和集
    task.set_cpu_set((1 << axconfig::SMP) - 1, 1, axconfig::SMP);

    task.reset_time_stat(current_time_nanos() as usize);

    //add_wait_for_exit_queue(&axtask);
    task
}

pub(crate) fn new_init_task(name: String) -> AxTaskRef {
    let inner = taskctx::TaskInner::new_init(
        name,
        #[cfg(feature = "tls")]
        tls_area(),
    );

    #[cfg(feature = "monolithic")]
    // 设置 CPU 亲和集
    inner.set_cpu_set((1 << axconfig::SMP) - 1, 1, axconfig::SMP);

    let axtask = Arc::new(AxTask::new(Task::new(
        0,
        Mutex::new(Arc::new(Mutex::new(MemorySet::new_empty()))),
        0,
        vec![None; FD_LIMIT_ORIGIN],
        inner,
    )));

    add_wait_for_exit_queue(&axtask);
    axtask
}
/// A wrapper of [`AxTaskRef`] as the current task.
pub struct CurrentTask(ManuallyDrop<AxTaskRef>);

impl CurrentTask {
    pub(crate) fn try_get() -> Option<Self> {
        let ptr: *const super::AxTask = taskctx::current_task_ptr();
        if !ptr.is_null() {
            Some(Self(unsafe { ManuallyDrop::new(AxTaskRef::from_raw(ptr)) }))
        } else {
            None
        }
    }

    pub(crate) fn get() -> Self {
        Self::try_get().expect("current task is uninitialized")
    }

    /// Converts [`CurrentTask`] to [`AxTaskRef`].
    pub fn as_task_ref(&self) -> &AxTaskRef {
        &self.0
    }

    pub(crate) fn clone(&self) -> AxTaskRef {
        self.0.deref().clone()
    }

    pub(crate) fn ptr_eq(&self, other: &AxTaskRef) -> bool {
        Arc::ptr_eq(&self.0, other)
    }

    pub(crate) unsafe fn init_current(init_task: AxTaskRef) {
        #[cfg(feature = "tls")]
        axhal::arch::write_thread_pointer(init_task.get_tls_ptr());
        let ptr = Arc::into_raw(init_task);
        taskctx::set_current_task_ptr(ptr);
    }

    pub(crate) unsafe fn set_current(prev: Self, next: AxTaskRef) {
        let Self(arc) = prev;
        ManuallyDrop::into_inner(arc); // `call Arc::drop()` to decrease prev task reference count.
        let ptr = Arc::into_raw(next);
        taskctx::set_current_task_ptr(ptr);
    }
}

impl Deref for CurrentTask {
    type Target = Task;
    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

extern "C" {
    fn task_entry();
}
