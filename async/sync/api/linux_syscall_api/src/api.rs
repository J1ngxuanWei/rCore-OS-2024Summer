extern crate alloc;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use axerrno::ax_err_type;
use axhal::{
    arch::{flush_tlb, write_page_table_root},
    KERNEL_PROCESS_ID,
};
use axtask::{wait_pid, yield_now_task};
use axruntime::KERNEL_PAGE_TABLE;
use axtask::{TaskId, EXITED_TASKS,task::TID2TASK};

use axfs::api::OpenFlags;

//use crate::syscall_task::syscall_kill;

/// 在完成一次系统调用之后，恢复全局目录
pub fn init_current_dir() {
    axfs::api::set_current_dir("/").expect("reset current dir failed");
}

/// Flags for opening a file
pub type FileFlags = OpenFlags;

/// 释放所有非内核进程
pub fn recycle_user_process() {
    let kernel_task = Arc::clone(TID2TASK.lock().get(&KERNEL_PROCESS_ID).unwrap());

    let childrens_num = kernel_task.children.lock().len();
    let tid2task = TID2TASK.lock();

    for index in 0..childrens_num {
        let childrenid = kernel_task.children.lock().get(index).unwrap().clone();
        let child= tid2task.get(&childrenid).unwrap();
        if !child.is_kernel_task() {
            // kill the process
            //let args: [usize; 6] = [pid as usize, 9, 0, 0, 0, 0];
            //let _ = syscall_kill(args);
            yield_now_task();
        }
    }


    kernel_task
        .children
        .lock()
        .retain(|x| *x == KERNEL_PROCESS_ID || tid2task.contains_key(x));

    drop(tid2task);

    TaskId::clear();
    unsafe {
        write_page_table_root(KERNEL_PAGE_TABLE.root_paddr());
        flush_tlb(None);
    };
    EXITED_TASKS.lock().clear();
    init_current_dir();
}

/// To read a file with the given path
pub fn read_file(path: &str) -> Option<String> {
    axfs::api::read_to_string(path).ok()
}

#[allow(unused)]
/// 分割命令行参数
fn get_args(command_line: &[u8]) -> Vec<String> {
    let mut args = Vec::new();
    // 需要判断是否存在引号，如busybox_cmd.txt的第一条echo指令便有引号
    // 若有引号时，不能把引号加进去，同时要注意引号内的空格不算是分割的标志
    let mut in_quote = false;
    let mut arg_start = 0; // 一个新的参数的开始位置
    for pos in 0..command_line.len() {
        if command_line[pos] == b'\"' {
            in_quote = !in_quote;
        }
        if command_line[pos] == b' ' && !in_quote {
            // 代表要进行分割
            // 首先要防止是否有空串
            if arg_start != pos {
                args.push(
                    core::str::from_utf8(&command_line[arg_start..pos])
                        .unwrap()
                        .to_string(),
                );
            }
            arg_start = pos + 1;
        }
    }
    // 最后一个参数
    if arg_start != command_line.len() {
        args.push(
            core::str::from_utf8(&command_line[arg_start..])
                .unwrap()
                .to_string(),
        );
    }
    args
}

/// To run a testcase with the given name and environment variables, which will be used in initproc
pub fn run_testcase(testcase: &str, envs: Vec<String>) {
    //axlog::ax_println!("Running testcase: {}", testcase);
    let args = get_args(testcase.as_bytes());
    let mut args_vec: Vec<String> = Vec::new();
    for arg in args {
        args_vec.push(arg.to_string());
    }

    let user_task = axtask::Task::init(args_vec, &envs).unwrap();
    let now_task_id = user_task.tid() as isize;
    let mut exit_code = 0;
    loop {
        if unsafe { wait_pid(now_task_id, &mut exit_code as *mut i32) }.is_ok() {
            break;
        }
        yield_now_task();
    }
    recycle_user_process();
    // axlog::ax_println!(
    //     "Testcase {} finished with exit code {}",
    //     testcase,
    //     exit_code
    // );
}
