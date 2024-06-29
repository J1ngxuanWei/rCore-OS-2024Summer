//! 提供和 task 模块相关的 syscall

mod task_syscall_id;

use crate::SyscallResult;
pub use task_syscall_id::TaskSyscallId::{self, *};

mod imp;

pub use imp::*;

/// 进行 syscall 的分发
pub fn task_syscall(syscall_id: task_syscall_id::TaskSyscallId, args: [usize; 6]) -> SyscallResult {
    match syscall_id {
        EXIT => syscall_exit(args),
        #[allow(unused)]
        _ => {
            panic!("Invalid Syscall Id: {:?}!", syscall_id);
            // return -1;
            // exit(-1)
        }
    }
}
