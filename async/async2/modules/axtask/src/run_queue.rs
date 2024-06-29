use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::arch::asm;

use lazy_init::LazyInit;
use scheduler::BaseScheduler;
use spinlock::SpinNoIrq;
use taskctx::TaskStack;
use taskctx::TaskState;

use crate::schedule::notify_wait_for_exit;
use crate::task::{new_init_task, new_task_inner, CurrentTask};
use crate::Mutex;
use crate::Task;
use crate::{AxTask, AxTaskRef, Scheduler, WaitQueue};
use alloc::vec;
use alloc::vec::Vec;
use axhal::arch::write_trapframe_to_kstack;
use axhal::arch::TrapFrame;
use axhal::mem::VirtAddr;
use axmem::MemorySet;

use crate::current;
use crate::get_max_pri_task_id;

use crate::ctx::Context;

pub static KSTACK_NUM: usize = 10;

// TODO: per-CPU
/// The running task-queue of the kernel.
pub static RUN_QUEUE: LazyInit<SpinNoIrq<AxRunQueue>> = LazyInit::new();

// TODO: per-CPU
/// The exited task-queue of the kernel.
pub static EXITED_TASKS: SpinNoIrq<VecDeque<AxTaskRef>> = SpinNoIrq::new(VecDeque::new());

static WAIT_FOR_EXIT: WaitQueue = WaitQueue::new();

#[percpu::def_percpu]
/// The idle task of the kernel.
pub static IDLE_TASK: LazyInit<AxTaskRef> = LazyInit::new();

#[allow(unused)]
/// The struct to define the running task-queue of the kernel.
pub struct AxRunQueue {
    scheduler: Scheduler,
    ctx: Mutex<Context>,
    kstack: Vec<TaskStack>,
    kstack_state: Vec<bool>,
    task_nub: usize,
    cur_stack: usize,
}

extern "C" {
    fn task_entry();
}

#[allow(unused)]
pub fn init_stack(s: &mut Vec<TaskStack>) {
    for i in 0..KSTACK_NUM {
        s.push(TaskStack::alloc(axconfig::TASK_STACK_SIZE));
    }
}

impl AxRunQueue {
    #[allow(unused)]
    pub fn new() -> SpinNoIrq<Self> {
        let gc_task_inner = new_task_inner(
            gc_entry,
            "gc".into(),
            axconfig::TASK_STACK_SIZE,
            #[cfg(feature = "monolithic")]
            0,
            #[cfg(feature = "monolithic")]
            false,
        );
        let mut gc_task = Task::new(
            0,
            Arc::new(Mutex::new(MemorySet::new_empty())),
            0,
            vec![],
            gc_task_inner,
        );
        #[cfg(feature = "tls")]
        let tls = VirtAddr::from(task.get_tls_ptr());
        #[cfg(not(feature = "tls"))]
        let tls = VirtAddr::from(0);
        let mut kssv: Vec<TaskStack> = vec![];
        init_stack(&mut kssv);
        let kss = kssv.get(0).unwrap();
        gc_task.init_tf(
            task_entry as usize,
            (kss.top().as_usize() - core::mem::size_of::<TrapFrame>()).into(),
            tls,
        );
        axlog::warn!(
            "Alloc 4 stack, stack0 top: {:x}, stack1 top: {:x}, stack2 top: {:x},stack3 top: {:x}",
            kssv[0].top().as_usize(),
            kssv[1].top().as_usize(),
            kssv[2].top().as_usize(),
            kssv[3].top().as_usize()
        );
        let gcc = Arc::new(AxTask::new(gc_task));
        let mut scheduler = Scheduler::new();
        //TODOWJX: gc先不添加和运行了
        //scheduler.add_task(gcc.clone());
        SpinNoIrq::new(Self {
            scheduler,
            ctx: Mutex::new(Context::default()),
            kstack: kssv,
            kstack_state: vec![false; KSTACK_NUM],
            task_nub: 0,
            cur_stack: 0,
        })
    }

    pub fn taskadd(&mut self) {
        self.task_nub += 1;
    }

    pub fn tasksub(&mut self) {
        self.task_nub -= 1;
    }

    pub fn get_kernel_stack_top(&self) -> usize {
        self.kstack[0].top().as_usize()
    }

    pub fn alloc_stack(&mut self) -> usize {
        // 从1开始，0是idle的栈
        for i in 1..KSTACK_NUM {
            if !self.kstack_state[i] {
                self.kstack_state[i] = true;
                axlog::warn!("alloc stack: {}", i);
                axlog::warn!("Now stack pool: {:?}", self.kstack_state);
                return i;
            }
        }
        panic!("no stack available");
    }

    pub fn free_stack(&mut self, idx: usize) {
        self.kstack_state[idx] = false;
        axlog::warn!("free stack: {}", idx);
        axlog::warn!("Now stack pool: {:?}", self.kstack_state);
    }

    pub fn get_idx_kernel_stack_top(&self, idx: usize) -> usize {
        self.kstack[idx].top().as_usize()
    }

    pub fn add_task(&mut self, task: AxTaskRef) {
        debug!("task spawn: {}", task.inner.lock().id_name());
        assert!(task.inner.lock().is_ready());
        self.scheduler.add_task(task);
    }

    #[cfg(feature = "irq")]
    pub fn scheduler_timer_tick(&mut self) {
        let curr = crate::current();
        if !curr.inner.lock().is_idle() && self.scheduler.task_tick(curr.as_task_ref()) {
            #[cfg(feature = "preempt")]
            curr.set_preempt_pending(true);
        }
    }

    pub fn yield_current(&mut self) {
        let curr = crate::current();
        trace!("task yield: {}", curr.inner.lock().id_name());
        assert!(curr.inner.lock().is_running());
        self.resched(false, 0);
    }

    pub fn set_current_priority(&mut self, prio: isize) -> bool {
        self.scheduler
            .set_priority(crate::current().as_task_ref(), prio)
    }

    #[cfg(feature = "preempt")]
    pub fn preempt_resched(&mut self) {
        let curr = crate::current();
        assert!(curr.is_running());

        // When we get the mutable reference of the run queue, we must
        // have held the `SpinNoIrq` lock with both IRQs and preemption
        // disabled. So we need to set `current_disable_count` to 1 in
        // `can_preempt()` to obtain the preemption permission before
        //  locking the run queue.
        let can_preempt = curr.can_preempt(1);

        debug!(
            "current task is to be preempted: {}, allow={}",
            curr.id_name(),
            can_preempt
        );
        if can_preempt {
            self.resched(true, 0);
        } else {
            curr.set_preempt_pending(true);
        }
    }

    pub fn exit_current(&mut self, exit_code: i32) {
        let curr = crate::current();
        debug!(
            "task exit: {}, exit_code={}",
            curr.inner.lock().id_name(),
            exit_code
        );
        assert!(curr.inner.lock().is_running());
        assert!(!curr.inner.lock().is_idle());
        if self.task_nub == 0 {
            axlog::warn!("task exit: all task exited, system halt!");
            EXITED_TASKS.lock().clear();
            axhal::misc::terminate();
        } else {
            curr.inner.lock().set_state(TaskState::Exited);
            curr.set_exit_code(exit_code);
            notify_wait_for_exit(curr.as_task_ref(), self);
            EXITED_TASKS.lock().push_back(curr.clone());
            WAIT_FOR_EXIT.notify_one_locked(false, self);
            self.resched(false, 1);
        }
        //unreachable!("task exited!");
    }

    #[cfg(feature = "monolithic")]
    /// 仅用于exec与exit时清除其他后台线程
    pub fn remove_task(&mut self, task: &AxTaskRef) {
        debug!("task remove: {}", task.inner.lock().id_name());
        // 当前任务不予清除
        // assert!(!task.is_running());
        assert!(!task.inner.lock().is_running());
        assert!(!task.inner.lock().is_idle());
        if task.inner.lock().is_ready() {
            task.inner.lock().set_state(TaskState::Exited);
            EXITED_TASKS.lock().push_back(task.clone());
            self.scheduler.remove_task(task);
        }
    }

    pub fn block_current<F>(&mut self, wait_queue_push: F)
    where
        F: FnOnce(AxTaskRef),
    {
        let curr = crate::current();
        debug!("task block: {}", curr.inner.lock().id_name());
        assert!(curr.inner.lock().is_running());
        assert!(!curr.inner.lock().is_idle());

        // we must not block current task with preemption disabled.
        #[cfg(feature = "preempt")]
        assert!(curr.can_preempt(1));

        curr.inner.lock().set_state(TaskState::Blocked);
        wait_queue_push(curr.clone());
        self.resched(false, 0);
    }

    pub fn unblock_task(&mut self, task: AxTaskRef, resched: bool) {
        debug!("task unblock: {}", task.inner.lock().id_name());
        if task.inner.lock().is_blocked() {
            task.inner.lock().set_state(TaskState::Ready);
            self.scheduler.add_task(task); // TODO: priority
            if resched {
                #[cfg(feature = "preempt")]
                crate::current().set_preempt_pending(true);
            }
        }
    }

    #[cfg(feature = "irq")]
    pub fn sleep_until(&mut self, deadline: axhal::time::TimeValue) {
        let curr = crate::current();
        debug!(
            "task sleep: {}, deadline={:?}",
            curr.inner.lock().id_name(),
            deadline
        );
        assert!(curr.inner.lock().is_running());
        assert!(!curr.inner.lock().is_idle());

        let now = axhal::time::current_time();
        if now < deadline {
            crate::timers::set_alarm_wakeup(deadline, curr.clone());
            curr.inner.lock().set_state(TaskState::Blocked);
            self.resched(false, 0);
        }
    }
}

impl AxRunQueue {
    pub fn run_task(&mut self, tid: u64) {
        self.resched(false, tid);
    }

    pub fn preempt_yield(&mut self, cid: u64) {
        let max_pri_tid = get_max_pri_task_id(cid);
        axlog::warn!("preempt yield: {}", max_pri_tid);
        if max_pri_tid != 0 {
            let mut o_tf = TrapFrame::default();
            let ttr: *mut TrapFrame = &mut o_tf;
            unsafe {
                asm!(
                    "
                    // save old context (callee-saved registers)
                    STR     ra, {ttr}, 35
                    STR     sp, {ttr}, 36
                    STR     s0, {ttr}, 37
                    STR     s1, {ttr}, 38
                    STR     s2, {ttr}, 39
                    STR     s3, {ttr}, 40
                    STR     s4, {ttr}, 41
                    STR     s5, {ttr}, 42
                    STR     s6, {ttr}, 43
                    STR     s7, {ttr}, 44
                    STR     s8, {ttr}, 45
                    STR     s9, {ttr}, 46
                    STR     s10, {ttr}, 47
                    STR     s11, {ttr}, 48
                    ",
                    ttr = in(reg) ttr,
                );
            }
            let flag = current().inner.lock().has_been_preempted;
            if flag {
                current().set_old_tf(o_tf);
                self.resched(true, max_pri_tid);
            }
        }
    }

    /// Common reschedule subroutine. If `preempt`, keep current task's time
    /// slice, otherwise reset it.
    pub fn resched(&mut self, preempt: bool, tid: u64) {
        let prev = crate::current();
        if prev.inner.lock().is_running() {
            prev.inner.lock().set_state(TaskState::Ready);
            if !prev.inner.lock().is_idle() {
                self.scheduler.put_prev_task(prev.clone(), preempt);
            }
        }
        #[cfg(feature = "monolithic")]
        {
            use alloc::collections::BTreeSet;
            use axhal::cpu::this_cpu_id;
            let mut task_set = BTreeSet::new();
            let next = loop {
                let task = self.scheduler.pick_next_task();
                if task.is_none() {
                    break unsafe {
                        // Safety: IRQs must be disabled at this time.
                        IDLE_TASK.current_ref_raw().get_unchecked().clone()
                    };
                }
                let task = task.unwrap();
                let tasktid = task.tid();
                if tasktid == 3 {
                    //先不选idle
                    continue;
                }
                // 原先队列有任务，但是全部不满足CPU适配集，则还是返回IDLE
                if task_set.contains(&tasktid) {
                    //如果有被抢占了的任务，也就是它开始了但是还没Exit，那么返回这个
                    if task.inner.lock().is_started {
                        break task;
                    }
                    break unsafe {
                        // Safety: IRQs must be disabled at this time.
                        IDLE_TASK.current_ref_raw().get_unchecked().clone()
                    };
                }

                let mask = task.inner.lock().get_cpu_set();
                let curr_cpu = this_cpu_id();
                // 如果当前进程没有被 vfork 阻塞，弹出任务
                if (mask & (1 << curr_cpu) != 0) && (tid == tasktid || tid == 0) {
                    break task;
                }
                task_set.insert(task.tid());
                self.scheduler.put_prev_task(task, false);
            };
            self.switch_to(prev, next);
        }
        #[cfg(not(feature = "monolithic"))]
        {
            let next = self.scheduler.pick_next_task().unwrap_or_else(|| unsafe {
                // Safety: IRQs must be disabled at this time.
                IDLE_TASK.current_ref_raw().get_unchecked().clone()
            });
            self.switch_to(prev, next);
        }
    }

    fn switch_to(&mut self, prev_task: CurrentTask, next_task: AxTaskRef) {
        axlog::warn!(
            "context switch: {} -> {}",
            prev_task.inner.lock().id_name(),
            next_task.inner.lock().id_name()
        );
        #[cfg(feature = "preempt")]
        next_task.inner.lock().set_preempt_pending(false);
        next_task.inner.lock().set_state(TaskState::Running);
        if prev_task.ptr_eq(&next_task) {
            return;
        }
        // 当任务进行切换时，更新两个任务的时间统计信息
        #[cfg(feature = "monolithic")]
        {
            let current_timestamp = axhal::time::current_time_nanos() as usize;
            next_task
                .inner
                .lock()
                .time_stat_when_switch_to(current_timestamp);
            prev_task
                .inner
                .lock()
                .time_stat_when_switch_from(current_timestamp);
        }
        unsafe {
            // The strong reference count of `prev_task` will be decremented by 1,
            // but won't be dropped until `gc_entry()` is called.
            assert!(Arc::strong_count(prev_task.as_task_ref()) > 1);
            assert!(Arc::strong_count(&next_task) >= 1);
            #[cfg(feature = "monolithic")]
            {
                let page_table_token = *next_task.inner.lock().page_table_token.get();
                if page_table_token != 0 {
                    axhal::arch::write_page_table_root0(page_table_token.into());
                }
            }
            let mut n_tf = next_task.get_tf().clone();
            //prev_task.set_tf(o_tf);
            //然后我们申请内核栈
            if next_task.is_kernel_task() {
                //内核进程就是main，不需要执行下面那么复杂的东西
                //然后我们需要将需要运行的trapframe写入到执行器的kstack中
                write_trapframe_to_kstack(self.get_kernel_stack_top(), &n_tf);
                //最后，设置当前指针，然后切换到新的任务
                CurrentTask::set_current(prev_task, next_task);
                self.resched(false, 0);
            } else {
                if next_task.inner.lock().is_started {
                    //axlog::warn!("has start");
                    //已经开始了，说明被更高优先级的抢占了，它已经有栈了
                    let idx = next_task.inner.lock().stack_idx;
                    self.cur_stack = idx;
                    next_task.inner.lock().has_been_preempted = false;
                    //然后我们需要将需要运行的trapframe写入到执行器的kstack中
                    //write_trapframe_to_kstack(self.get_idx_kernel_stack_top(idx), &n_tf);
                    //最后，设置当前指针，然后切换到新的任务
                    CurrentTask::set_current(prev_task, next_task);
                    let ttr: *mut TrapFrame = &mut n_tf;
                    asm!(
                        "
                        // restore new context
                        LDR     ra, {ttr}, 35
                        LDR     sp, {ttr}, 36
                        LDR     s0, {ttr}, 37
                        LDR     s1, {ttr}, 38
                        LDR     s2, {ttr}, 39
                        LDR     s3, {ttr}, 40
                        LDR     s4, {ttr}, 41
                        LDR     s5, {ttr}, 42
                        LDR     s6, {ttr}, 43
                        LDR     s7, {ttr}, 44
                        LDR     s8, {ttr}, 45
                        LDR     s9, {ttr}, 46
                        LDR     s10, {ttr}, 47
                        LDR     s11, {ttr}, 48
                        ret
                        ",
                        ttr = in(reg) ttr,
                    );
                } else {
                    //没有开始过，我们需要开始，从堆栈池里拿一个栈出来
                    let idx = self.alloc_stack();
                    self.cur_stack = idx;
                    next_task.inner.lock().is_started = true;
                    next_task.inner.lock().stack_idx = idx;
                    let kstack_top = self.kstack[idx].top();
                    next_task.tf.lock().kernel_sp = kstack_top.as_usize();
                    //然后我们需要将需要运行的trapframe写入到执行器的kstack中
                    write_trapframe_to_kstack(self.get_idx_kernel_stack_top(idx), &n_tf);
                    //最后，设置当前指针，然后切换到新的任务
                    CurrentTask::set_current(prev_task, next_task);
                }
            }
        }
    }
}

fn gc_entry() {
    loop {
        // Drop all exited tasks and recycle resources.
        let n = EXITED_TASKS.lock().len();
        for _ in 0..n {
            // Do not do the slow drops in the critical section.
            let task = EXITED_TASKS.lock().pop_front();
            if let Some(task) = task {
                if Arc::strong_count(&task) == 1 {
                    // If I'm the last holder of the task, drop it immediately.
                    drop(task);
                } else {
                    // Otherwise (e.g, `switch_to` is not compeleted, held by the
                    // joiner, etc), push it back and wait for them to drop first.
                    EXITED_TASKS.lock().push_back(task);
                }
            }
        }
        WAIT_FOR_EXIT.wait();
    }
}

pub(crate) fn init() {
    let main_task = new_init_task("main".into());
    //axlog::warn!("init task: {}", main_task.inner.lock().id_name());
    #[cfg(feature = "monolithic")]
    main_task.inner.lock().set_state(TaskState::Running);
    RUN_QUEUE.init_by(AxRunQueue::new());
    unsafe { CurrentTask::init_current(main_task) }
}

pub(crate) fn init_secondary() {
    let idle_task = new_init_task("idle".into()); // FIXME: name 现已被用作 prctl 使用的程序名，应另选方式判断 idle 进程
    #[cfg(feature = "monolithic")]
    idle_task.inner.lock().set_state(TaskState::Running);
    IDLE_TASK.with_current(|i| i.init_by(idle_task.clone()));
    unsafe { CurrentTask::init_current(idle_task) }
}
