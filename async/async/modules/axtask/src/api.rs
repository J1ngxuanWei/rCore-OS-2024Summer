//! Task APIs for multi-task configuration.

use alloc::{string::String, sync::Arc};
#[cfg(feature = "monolithic")]
use axhal::KERNEL_PROCESS_ID;
use taskctx::TaskState;

use crate::flags::WaitStatus;
use crate::link::real_path;
use crate::Mutex;
use alloc::{string::ToString, vec, vec::Vec};
use axconfig::{MAX_USER_HEAP_SIZE, MAX_USER_STACK_SIZE, USER_HEAP_BASE, USER_STACK_TOP};
use axerrno::{AxError, AxResult};
use axhal::mem::VirtAddr;
use axhal::paging::MappingFlags;
use axhal::time::{current_time_nanos, NANOS_PER_MICROS, NANOS_PER_SEC};
use axmem::MemorySet;
use core::ops::Deref;
use core::ptr::copy_nonoverlapping;
use core::str::from_utf8;
use axhal::arch::TrapFrame;
use core::{char, panic};
use elf_parser::{
    get_app_stack_region, get_auxv_vector, get_elf_entry, get_elf_segments, get_relocate_pairs,
};
use xmas_elf::program::SegmentData;

pub(crate) use crate::run_queue::{AxRunQueue, IDLE_TASK, RUN_QUEUE};

use crate::task::TID2TASK;

use crate::schedule::get_wait_for_exit_queue;
#[doc(cfg(feature = "multitask"))]
pub use crate::task::{new_task_inner, CurrentTask, Task};
#[doc(cfg(feature = "multitask"))]
pub use crate::wait_queue::WaitQueue;

/// The reference type of a task.
pub type AxTaskRef = Arc<AxTask>;

cfg_if::cfg_if! {
    if #[cfg(feature = "sched_rr")] {
        const MAX_TIME_SLICE: usize = 5;
        pub(crate) type AxTask = scheduler::RRTask<Task, MAX_TIME_SLICE>;
        pub(crate) type Scheduler = scheduler::RRScheduler<Task, MAX_TIME_SLICE>;
    } else if #[cfg(feature = "sched_cfs")] {
        pub(crate) type AxTask = scheduler::CFSTask<Task>;
        pub(crate) type Scheduler = scheduler::CFScheduler<Task>;
    } else {
        // If no scheduler features are set, use FIFO as the default.
        pub(crate) type AxTask = scheduler::FifoTask<Task>;
        pub(crate) type Scheduler = scheduler::FifoScheduler<Task>;
    }
}

/// Gets the current task, or returns [`None`] if the current task is not
/// initialized.
pub fn current_may_uninit() -> Option<CurrentTask> {
    CurrentTask::try_get()
}

/// 返回应用程序入口，用户栈底，用户堆底
pub fn load_app(
    name: String,
    mut args: Vec<String>,
    envs: &Vec<String>,
    memory_set: &mut MemorySet,
) -> AxResult<(VirtAddr, VirtAddr, VirtAddr)> {
    if name.ends_with(".sh") {
        args = [vec![String::from("busybox"), String::from("sh")], args].concat();
        return load_app("busybox".to_string(), args, envs, memory_set);
    }
    let elf_data = if let Ok(ans) = axfs::api::read(name.as_str()) {
        ans
    } else {
        // exit(0)
        return Err(AxError::NotFound);
    };
    let elf = xmas_elf::ElfFile::new(&elf_data).expect("Error parsing app ELF file.");
    debug!("app elf data length: {}", elf_data.len());
    if let Some(interp) = elf
        .program_iter()
        .find(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Interp))
    {
        let interp = match interp.get_data(&elf) {
            Ok(SegmentData::Undefined(data)) => data,
            _ => panic!("Invalid data in Interp Elf Program Header"),
        };

        let interp_path = from_utf8(interp).expect("Interpreter path isn't valid UTF-8");
        // remove trailing '\0'
        let interp_path = interp_path.trim_matches(char::from(0)).to_string();
        let real_interp_path = real_path(&interp_path);
        args = [vec![real_interp_path.clone()], args].concat();
        return load_app(real_interp_path, args, envs, memory_set);
    }
    info!("args: {:?}", args);
    let elf_base_addr = Some(0x400_0000);
    axlog::warn!("The elf base addr may be different in different arch!");
    // let (entry, segments, relocate_pairs) = parse_elf(&elf, elf_base_addr);
    let entry = get_elf_entry(&elf, elf_base_addr);
    let segments = get_elf_segments(&elf, elf_base_addr);
    let relocate_pairs = get_relocate_pairs(&elf, elf_base_addr);
    for segment in segments {
        memory_set.new_region(
            segment.vaddr,
            segment.size,
            segment.flags,
            segment.data.as_deref(),
            None,
        );
    }

    for relocate_pair in relocate_pairs {
        let src: usize = relocate_pair.src.into();
        let dst: usize = relocate_pair.dst.into();
        let count = relocate_pair.count;
        unsafe { copy_nonoverlapping(src.to_ne_bytes().as_ptr(), dst as *mut u8, count) }
    }

    // Now map the stack and the heap
    let heap_start = VirtAddr::from(USER_HEAP_BASE);
    let heap_data = [0_u8].repeat(MAX_USER_HEAP_SIZE);
    memory_set.new_region(
        heap_start,
        MAX_USER_HEAP_SIZE,
        MappingFlags::READ | MappingFlags::WRITE | MappingFlags::USER,
        Some(&heap_data),
        None,
    );
    info!(
        "[new region] user heap: [{:?}, {:?})",
        heap_start,
        heap_start + MAX_USER_HEAP_SIZE
    );

    let auxv = get_auxv_vector(&elf, elf_base_addr);

    let stack_top = VirtAddr::from(USER_STACK_TOP);
    let stack_size = MAX_USER_STACK_SIZE;

    let (stack_data, stack_bottom) = get_app_stack_region(args, envs, auxv, stack_top, stack_size);
    memory_set.new_region(
        stack_top,
        stack_size,
        MappingFlags::USER | MappingFlags::READ | MappingFlags::WRITE,
        Some(&stack_data),
        None,
    );
    info!(
        "[new region] user stack: [{:?}, {:?})",
        stack_top,
        stack_top + stack_size
    );
    Ok((entry, stack_bottom.into(), heap_start))
}

extern "C" {
    fn task_entry();
}

/// 初始化内核调度进程
pub fn init_kernel_task() {
    crate::executor::task_future_init();
    crate::init_scheduler();
    const IDLE_TASK_STACK_SIZE: usize = 4096;
    let idle_task_inner = new_task_inner(
        || crate::run_idle(),
        "idle".into(), // FIXME: name 现已被用作 prctl 使用的程序名，应另选方式判断 idle 进程
        IDLE_TASK_STACK_SIZE,
        #[cfg(feature = "monolithic")]
        0,
        #[cfg(feature = "monolithic")]
        false,
    );
    let mut idle_task = Task::new(
        0,
        Arc::new(Mutex::new(MemorySet::new_empty())),
        0,
        vec![],
        idle_task_inner,
    );
    #[cfg(feature = "tls")]
    let tls = VirtAddr::from(task.get_tls_ptr());
    #[cfg(not(feature = "tls"))]
    let tls = VirtAddr::from(0);
    idle_task.init_tf(
        task_entry as usize,
        (RUN_QUEUE.lock().get_kernel_stack_top()- core::mem::size_of::<TrapFrame>()).into(),
        tls,
    );
    IDLE_TASK.with_current(|i| i.init_by(Arc::new(AxTask::new(idle_task))));
    unsafe {
        TID2TASK
            .lock()
            .insert(1, Arc::clone(IDLE_TASK.current_ref_raw().get_unchecked()));
    }
}

/// 退出当前任务
pub fn exit_current_task(exit_code: i32) -> ! {
    let current_task = current();

    let curr_id = current_task.tid();

    info!("exit task id {} with code _{}_", curr_id, exit_code);

    // clear_child_tid 的值不为 0，则将这个用户地址处的值写为0
    let clear_child_tid = current_task.inner.lock().get_clear_child_tid();
    if clear_child_tid != 0 {
        // 先确认是否在用户空间
        if current_task
            .manual_alloc_for_lazy(clear_child_tid.into())
            .is_ok()
        {
            unsafe {
                *(clear_child_tid as *mut i32) = 0;
            }
        }
    }

    current_task.set_exit_code(exit_code);

    current_task.set_zombie(true);

    current_task.fd_manager.fd_table.lock().clear();
    let tid2ta = TID2TASK.lock();
    let kernel_task = tid2ta.get(&KERNEL_PROCESS_ID).unwrap();
    // 将子进程交给idle进程
    // process.memory_set = Arc::clone(&kernel_process.memory_set);
    for childid in current_task.children.lock().deref() {
        let child = tid2ta.get(childid).unwrap();
        child.set_parent(KERNEL_PROCESS_ID);
        kernel_task.children.lock().push(child.tid());
    }
    drop(tid2ta);
    TID2TASK.lock().remove(&curr_id);

    drop(current_task);
    RUN_QUEUE.lock().tasksub();
    RUN_QUEUE.lock().exit_current(exit_code);
}

/// 当从内核态到用户态时，统计对应进程的时间信息
pub fn time_stat_from_kernel_to_user() {
    let curr_task = current();
    curr_task
        .inner
        .lock()
        .time_stat_from_kernel_to_user(current_time_nanos() as usize);
}

#[no_mangle]
/// 当从用户态到内核态时，统计对应进程的时间信息
pub fn time_stat_from_user_to_kernel() {
    let curr_task = current();
    curr_task
        .inner
        .lock()
        .time_stat_from_user_to_kernel(current_time_nanos() as usize);
}

/// 统计时间输出
/// (用户态秒，用户态微秒，内核态秒，内核态微秒)
pub fn time_stat_output() -> (usize, usize, usize, usize) {
    let curr_task = current();
    let (utime_ns, stime_ns) = curr_task.inner.lock().time_stat_output();
    (
        utime_ns / NANOS_PER_SEC as usize,
        utime_ns / NANOS_PER_MICROS as usize,
        stime_ns / NANOS_PER_SEC as usize,
        stime_ns / NANOS_PER_MICROS as usize,
    )
}

/// To deal with the page fault
pub fn handle_page_fault(addr: VirtAddr, flags: MappingFlags) {
    axlog::debug!("'page fault' addr: {:?}, flags: {:?}", addr, flags);
    let current_task = current();
    axlog::debug!(
        "memory token : {:#x}",
        current_task.memory_set.lock().page_table_token()
    );

    if current_task
        .memory_set
        .lock()
        .handle_page_fault(addr, flags)
        .is_ok()
    {
        axhal::arch::flush_tlb(None);
    } else {
        panic!("handle page fault failed");
    }
}

/// 在当前进程找对应的子task，并等待子task结束
/// 若找到了则返回对应的pid
/// 否则返回一个状态
///
/// # Safety
///
/// 保证传入的 ptr 是有效的
pub unsafe fn wait_pid(tid: isize, exit_code_ptr: *mut i32) -> Result<u64, WaitStatus> {
    // 获取当前进程
    let curr_task = current();
    let mut exit_task_id: usize = 0;
    let mut answer_id: u64 = 0;
    let mut answer_status = WaitStatus::NotExist;
    for (index, childid) in curr_task.children.lock().iter().enumerate() {
        let tid2task = TID2TASK.lock();
        let child = tid2task.get(childid).unwrap();
        if tid == -1 {
            // 任意一个task结束都可以的
            answer_status = WaitStatus::Running;
            if let Some(exit_code) = child.get_code_if_exit() {
                answer_status = WaitStatus::Exited;
                info!("wait tid _{}_ with code _{}_", child.tid(), exit_code);
                exit_task_id = index;
                if !exit_code_ptr.is_null() {
                    unsafe {
                        // 因为没有切换页表，所以可以直接填写
                        *exit_code_ptr = exit_code << 8;
                    }
                }
                answer_id = child.tid();
                break;
            }
        } else if *childid == tid as u64 {
            // 找到了对应的进程
            if let Some(exit_code) = child.get_code_if_exit() {
                answer_status = WaitStatus::Exited;
                info!("wait pid _{}_ with code _{:?}_", child.tid(), exit_code);
                exit_task_id = index;
                if !exit_code_ptr.is_null() {
                    unsafe {
                        *exit_code_ptr = exit_code << 8;
                        // 用于WEXITSTATUS设置编码
                    }
                }
                answer_id = child.tid();
            } else {
                answer_status = WaitStatus::Running;
            }
            break;
        }
    }
    // 若task成功结束，需要将其从父task的children中删除
    if answer_status == WaitStatus::Exited {
        curr_task.children.lock().remove(exit_task_id);
        return Ok(answer_id);
    }
    Err(answer_status)
}

/// 以进程作为中转调用task的yield
pub fn yield_now_task() {
    crate::yield_now();
}

/// 以进程作为中转调用task的sleep
pub fn sleep_now_task(dur: core::time::Duration) {
    crate::sleep(dur);
}

/// current running task
pub fn current_task() -> CurrentTask {
    crate::current()
}

/// 设置当前任务的clear_child_tid
pub fn set_child_tid(tid: usize) {
    let curr = current_task();
    curr.inner.lock().set_clear_child_tid(tid);
}

/// Get the task reference by tid
pub fn get_task_ref(tid: u64) -> Option<AxTaskRef> {
    TID2TASK.lock().get(&tid).cloned()
}

/// Gets the current task.
///
/// # Panics
///
/// Panics if the current task is not initialized.
pub fn current() -> CurrentTask {
    CurrentTask::get()
}

/// Initializes the task scheduler (for the primary CPU).
pub fn init_scheduler() {
    info!("Initialize scheduling...");
    crate::run_queue::init();
    #[cfg(feature = "irq")]
    crate::timers::init();

    info!("  use {} scheduler.", Scheduler::scheduler_name());
}

/// Initializes the task scheduler for secondary CPUs.
pub fn init_scheduler_secondary() {
    crate::run_queue::init_secondary();
}

/// Handles periodic timer ticks for the task manager.
///
/// For example, advance scheduler states, checks timed events, etc.
#[cfg(feature = "irq")]
#[doc(cfg(feature = "irq"))]
pub fn on_timer_tick() {
    crate::timers::check_events();
    RUN_QUEUE.lock().scheduler_timer_tick();
}

#[cfg(feature = "preempt")]
/// Checks if the current task should be preempted.
pub fn current_check_preempt_pending() {
    let curr = crate::current();
    if curr.get_preempt_pending() && curr.can_preempt(0) {
        let mut rq = crate::RUN_QUEUE.lock();
        if curr.get_preempt_pending() {
            rq.preempt_resched();
        }
    }
}

/// Set the priority for current task.
///
/// The range of the priority is dependent on the underlying scheduler. For
/// example, in the [CFS] scheduler, the priority is the nice value, ranging from
/// -20 to 19.
///
/// Returns `true` if the priority is set successfully.
///
/// [CFS]: https://en.wikipedia.org/wiki/Completely_Fair_Scheduler
pub fn set_priority(prio: isize) -> bool {
    RUN_QUEUE.lock().set_current_priority(prio)
}

/// Current task gives up the CPU time voluntarily, and switches to another
/// ready task.
pub fn yield_now() {
    RUN_QUEUE.lock().yield_current();
}

/// Current task is going to sleep for the given duration.
///
/// If the feature `irq` is not enabled, it uses busy-wait instead.
pub fn sleep(dur: core::time::Duration) {
    sleep_until(axhal::time::current_time() + dur);
}

/// Current task is going to sleep, it will be woken up at the given deadline.
///
/// If the feature `irq` is not enabled, it uses busy-wait instead.
pub fn sleep_until(deadline: axhal::time::TimeValue) {
    #[cfg(feature = "irq")]
    RUN_QUEUE.lock().sleep_until(deadline);
    #[cfg(not(feature = "irq"))]
    axhal::time::busy_wait_until(deadline);
}

/// Current task is going to sleep, it will be woken up when the given task exits.
///
/// If the given task is already exited, it will return immediately.
pub fn join(task: &AxTaskRef) -> Option<i32> {
    get_wait_for_exit_queue(task)
        .map(|wait_queue| wait_queue.wait_until(|| task.inner.lock().state() == TaskState::Exited));
    Some(task.get_exit_code())
}

#[cfg(feature = "monolithic")]
/// Current task is going to sleep. It will be woken up when the given task does exec syscall or exit.
pub fn vfork_suspend(task: &AxTaskRef) {
    get_wait_for_exit_queue(task).map(|wait_queue| {
        wait_queue.wait_until(|| {
            // If the given task does the exec syscall, it will be the leader of the new process.
            task.inner.lock().state() == TaskState::Exited
        });
    });
}

#[cfg(feature = "monolithic")]
/// To wake up the task that is blocked because vfork out of current task
pub fn wake_vfork_process(task: &AxTaskRef) {
    get_wait_for_exit_queue(task).map(|wait_queue| wait_queue.notify_all(true));
}

/// Exits the current task.
pub fn exit(exit_code: i32) -> ! {
    RUN_QUEUE.lock().exit_current(exit_code)
}

/// The idle task routine.
///
/// It runs an infinite loop that keeps calling [`yield_now()`].
pub fn run_idle() -> ! {
    loop {
        yield_now();
        debug!("idle task: waiting for IRQs...");
        #[cfg(feature = "irq")]
        axhal::arch::wait_for_irqs();
    }
}
