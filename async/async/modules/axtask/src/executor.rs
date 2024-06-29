use alloc::collections::VecDeque;
use async_task::{Runnable, ScheduleInfo, Task, WithInfo};
use core::future::Future;
use spinlock::SpinNoIrq;

use crate::future::UserTaskFuture;
use crate::future::YieldFuture;
use crate::AxTaskRef;

use axhal::arch::TrapFrame;

struct TaskQueue {
    queue: SpinNoIrq<Option<VecDeque<Runnable>>>,
}

impl TaskQueue {
    pub const fn new() -> Self {
        Self {
            queue: SpinNoIrq::new(None),
        }
    }

    pub fn init(&self) {
        *self.queue.lock() = Some(VecDeque::new());
    }

    pub fn push(&self, runnable: Runnable) {
        let mut lock = self.queue.lock();
        lock.as_mut().unwrap().push_back(runnable);
    }

    pub fn push_preempt(&self, runnable: Runnable) {
        self.queue.lock().as_mut().unwrap().push_front(runnable);
    }

    pub fn fetch(&self) -> Option<Runnable> {
        self.queue.lock().as_mut().unwrap().pop_front()
    }
}

static TASK_QUEUE: TaskQueue = TaskQueue::new();

pub fn task_future_init() {
    TASK_QUEUE.init();
}

/// Add a task into task queue
pub fn spawn<F>(future: F) -> (Runnable, Task<F::Output>)
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let schedule = move |runnable: Runnable, info: ScheduleInfo| {
        // TASK_QUEUE.push(runnable);
        if info.woken_while_running {
            // i.e `yield_now()`
            // log::error!("yield now");
            TASK_QUEUE.push(runnable);
        } else {
            // i.e. woken up by some signal
            TASK_QUEUE.push_preempt(runnable);
        }
    };
    async_task::spawn(future, WithInfo(schedule))
}

/// Return the number of the tasks executed
pub fn run_all() -> usize {
    let mut n = 0;
    loop {
        if let Some(task) = TASK_QUEUE.fetch() {
            // info!("fetch a task");
            task.run();
            n += 1;
        } else {
            break;
        }
    }
    n
}

/// Yield the current thread (and the scheduler will switch to next thread)
pub async fn yield_now() {
    YieldFuture(false).await;
}

/// Spawn a new user thread
pub fn spawn_user_task(task: AxTaskRef) {
    // let future = schedule::OutermostFuture::new(thread.clone(), async {});
    let future = UserTaskFuture::new(task.clone(), task_loop());
    let (runnable, task) = spawn(future);
    runnable.schedule();
    task.detach();
}

pub async fn task_loop() {
    unsafe { crate::RUN_QUEUE.force_unlock() };
    let task = crate::current();
    let mut tf = task.get_tf();
    loop {
        // 切换页表已经在switch实现了
        // 记得更新时间
        task.inner
            .lock()
            .time_stat_from_kernel_to_user(axhal::time::current_time_nanos() as usize);
        // return to user space
        unsafe {
            riscv_trap_return(&mut tf);
            // next time when user traps into kernel, it will come back here
            riscv_trap_handler(&mut tf, true);
        }
        if task.get_zombie() {
            break;
        }
        task.set_tf(tf);
    }
}

extern "C" {
    fn riscv_trap_return(tf: &mut TrapFrame);
    fn riscv_trap_handler(tf: &mut TrapFrame, from_user: bool);
}

