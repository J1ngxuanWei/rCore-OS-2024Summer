//! 文件系统相关系统调用

mod ctype;
pub mod imp;

use crate::SyscallResult;
use axerrno::AxResult;
use axfs::api::{File, OpenFlags};
pub use ctype::FileDesc;
mod fs_syscall_id;
pub use fs_syscall_id::FsSyscallId::{self, *};
extern crate alloc;
use imp::*;

/// 若使用多次new file打开同名文件，那么不同new file之间读写指针不共享，但是修改的内容是共享的
pub fn new_file(path: &str, flags: &OpenFlags) -> AxResult<File> {
    let mut file = File::options();
    file.read(flags.readable());
    file.write(flags.writable());
    file.create(flags.creatable());
    file.create_new(flags.new_creatable());
    file.open(path)
}

/// 文件系统相关系统调用
pub fn fs_syscall(syscall_id: fs_syscall_id::FsSyscallId, args: [usize; 6]) -> SyscallResult {
    match syscall_id {
        OPENAT => syscall_openat(args),
        CLOSE => syscall_close(args),
        READ => syscall_read(args),
        WRITE => syscall_write(args),
    }
}
