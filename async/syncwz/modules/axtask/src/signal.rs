//! 负责处理进程中与信号相关的内容
extern crate alloc;
use crate::Mutex;
use crate::{TaskState, RUN_QUEUE};
use alloc::sync::Arc;
use axerrno::{AxError, AxResult};
use axhal::arch::{read_trapframe_from_kstack, write_trapframe_to_kstack, TrapFrame};
use axsignal::{ucontext::SignalUserContext, SignalHandler, SignalSet};

/// 信号处理模块，进程间不共享
pub struct SignalModule {
    /// 是否存在siginfo
    pub sig_info: bool,
    /// 保存的trap上下文
    pub last_trap_frame_for_signal: Option<TrapFrame>,
    /// 信号处理函数集
    pub signal_handler: Arc<Mutex<SignalHandler>>,
    /// 未决信号集
    pub signal_set: SignalSet,
}

impl SignalModule {
    /// 初始化信号模块
    pub fn init_signal(signal_handler: Option<Arc<Mutex<SignalHandler>>>) -> Self {
        let signal_handler =
            signal_handler.unwrap_or_else(|| Arc::new(Mutex::new(SignalHandler::new())));
        let signal_set = SignalSet::new();
        let last_trap_frame_for_signal = None;
        let sig_info = false;
        Self {
            sig_info,
            last_trap_frame_for_signal,
            signal_handler,
            signal_set,
        }
    }
}

use crate::{current_task,task::TID2TASK};

/// 将保存的trap上下文填入内核栈中
///
/// 若使用了SIG_INFO，此时会对原有trap上下文作一定修改。
///
/// 若确实存在可以被恢复的trap上下文，则返回true
#[no_mangle]
pub fn load_trap_for_signal() -> bool {
    let current_task = current_task();

    let mut signal_modules = current_task.signal_modules.lock();
    let signal_module = signal_modules.get_mut(&current_task.tid()).unwrap();
    if let Some(old_trap_frame) = signal_module.last_trap_frame_for_signal.take() {
        unsafe {
            // let now_trap_frame: *mut TrapFrame = current_task.get_first_trap_frame();
            let mut now_trap_frame =
                read_trapframe_from_kstack(RUN_QUEUE.lock().get_kernel_stack_top());
            // 考虑当时调用信号处理函数时，sp对应的地址上的内容即是SignalUserContext
            // 此时认为一定通过sig_return调用这个函数
            // 所以此时sp的位置应该是SignalUserContext的位置
            let sp = now_trap_frame.get_sp();
            now_trap_frame = old_trap_frame;
            if signal_module.sig_info {
                let pc = (*(sp as *const SignalUserContext)).get_pc();
                now_trap_frame.set_pc(pc);
            }
            write_trapframe_to_kstack(RUN_QUEUE.lock().get_kernel_stack_top(), &now_trap_frame);
        }
        true
    } else {
        false
    }
}


/// 处理当前进程的信号
///
/// 若返回值为真，代表需要进入处理信号，因此需要执行trap的返回
pub fn handle_signals() {}

/// 从信号处理函数返回
///
/// 返回的值与原先syscall应当返回的值相同，即返回原先保存的trap上下文的a0的值
pub fn signal_return() -> isize {
    if load_trap_for_signal() {
        // 说明确实存在着信号处理函数的trap上下文
        // 此时内核栈上存储的是调用信号处理前的trap上下文
        read_trapframe_from_kstack(RUN_QUEUE.lock().get_kernel_stack_top()).get_ret_code() as isize
    } else {
        // 没有进行信号处理，但是调用了sig_return
        // 此时直接返回-1
        -1
    }
}

/// 发送信号到指定的线程
pub fn send_signal_to_thread(tid: isize, signum: isize) -> AxResult<()> {
    let tid2task = TID2TASK.lock();
    let task = if let Some(task) = tid2task.get(&(tid as u64)) {
        Arc::clone(task)
    } else {
        return Err(AxError::NotFound);
    };
    drop(tid2task);
    let mut signal_modules = task.signal_modules.lock();
    if !signal_modules.contains_key(&(tid as u64)) {
        return Err(axerrno::AxError::NotFound);
    }
    let signal_module = signal_modules.get_mut(&(tid as u64)).unwrap();
    signal_module.signal_set.try_add_signal(signum as usize);
    // 如果这个时候对应的线程是处于休眠状态的，则唤醒之，进入信号处理阶段
    drop(signal_modules);
    if task.inner.lock().state() == TaskState::Blocked {
        RUN_QUEUE.lock().unblock_task(task, false);
    }
    Ok(())
}

/// Whether the current process has signals pending
pub fn current_have_signals() -> bool {
    //current_task().have_signals().is_some()
    true
}
