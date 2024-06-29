//! The userboot of the operating system, which will start the first user process and go into the user mode
#![cfg_attr(not(test), no_std)]
#![no_main]

extern crate alloc;
use alloc::format;
use alloc::boxed::Box;

/// 初赛测例
#[allow(dead_code)]
const JUNIOR_TESTCASES: &[&str] = &[
    "brk",
    "chdir",
    "clone",
    "close",
    "dup",
    "dup2",
    "execve",
    "exit",
    "fork",
    "fstat",
    "getcwd",
    "getdents",
    "getpid",
    "getppid",
    "gettimeofday",
    "mkdir_",
    "mmap",
    "mount",
    "munmap",
    "open",
    "openat",
    "pipe",
    "read",
    "sleep",
    "times",
    "umount",
    "uname",
    "unlink",
    "wait",
    "waitpid",
    "write",
    "yield",
];

/// 初赛测例
#[allow(dead_code)]
const TEST_TESTCASES: &[&str] = &[
    //"open",
    "openat",
];

#[allow(unused)]
pub fn run_batch_testcases() {
    let mut test_iter=Box::new(TEST_TESTCASES.iter());

    axcomp::run_testcase(test_iter);
}

#[no_mangle]
fn main() {
    axcomp::fs_init();
    run_batch_testcases();
    axcomp::println(format!("System halted with exit code {}", 0).as_str());
}
