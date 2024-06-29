//! This crate provides a safe interface to the Linux syscall API for Starry modules.
#![cfg_attr(all(not(test), not(doc)), no_std)]
#![feature(stmt_expr_attributes)]
#![allow(unused)]
mod ctypes;
use ctypes::*;
mod syscall;
mod syscall_fs;
mod syscall_task;

pub use axfs::api::{File, OpenFlags}; 
pub use axtask::link::{create_link, FilePath};
pub use syscall_fs::new_file;

mod api;
pub use api::*;



// These interfaces is exposed to the trap handler
pub mod trap;

extern crate alloc;
/// 需要手动引入这个库，否则会报错：`#[panic_handler]` function required, but not found.
extern crate axruntime;

/// The error of a syscall, which is a `LinuxError`
pub type SyscallError = axerrno::LinuxError;

/// The result of a syscall
///
/// * `Ok(x)` - The syscall is successful, and the return value is `x`
///
/// * `Err(error)` - The syscall failed, and the error is related to `linux_error`
pub type SyscallResult = Result<isize, SyscallError>;

/// Accept the result of a syscall, and return the isize to the user
pub(crate) fn deal_result(result: SyscallResult) -> isize {
    match result {
        Ok(x) => x,
        Err(error) => -(error.code() as isize),
    }
}

pub fn yield_to_test(){
    axtask::yield_now_task();
}