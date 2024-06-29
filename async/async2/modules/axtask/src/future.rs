use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::AxTaskRef;
use crate::RUN_QUEUE;

pub struct YieldFuture(pub bool);

impl Future for YieldFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        // Wake up this future, which means putting this thread into
        // the tail of the task queue
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

#[allow(unused)]
pub struct UserTaskFuture<F: Future + Send + 'static> {
    task: AxTaskRef,
    task_future: F,
}

impl<F: Future + Send + 'static> UserTaskFuture<F> {
    #[inline]
    pub fn new(taska: AxTaskRef, future: F) -> Self {
        Self {
            task: taska.clone(),
            task_future: future,
        }
    }
}

impl<F: Future + Send + 'static> Future for UserTaskFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // There are 2 cases that are safe:
        // 1. the outermost future itself is unpin
        // 2. the outermost future isn't unpin but we make sure that it won't be moved
        // SAFETY: although getting the mut ref of a pin type is unsafe,
        // we only need to change the task_ctx, which is ok
        let this = unsafe { self.get_unchecked_mut() };

        let tid = this.task.tid();
        RUN_QUEUE.lock().run_task(tid);

        // run the `threadloop`
        // SAFETY:
        // the task future(i.e. threadloop) won't be moved.
        // One way to avoid unsafe is to wrap the task_future in
        // a Mutex<Pin<Box<>>>>, which requires locking for every polling
        let ret = unsafe { Pin::new_unchecked(&mut this.task_future).poll(cx) };

        ret
    }
}
