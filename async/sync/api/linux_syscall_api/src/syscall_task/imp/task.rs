use crate::SyscallResult;
use axtask::exit_current_task;

use axlog::info;

extern crate alloc;

// pub static TEST_FILTER: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

/// # Arguments
/// * `exit_code` - i32
pub fn syscall_exit(args: [usize; 6]) -> SyscallResult {
    let exit_code = args[0] as i32;
    info!("exit: exit_code = {}", exit_code);
    // let cases = ["fcanf", "fgetwc_buffering", "lat_pipe"];
    // let mut test_filter = TEST_FILTER.lock();
    // for case in cases {
    //     let case = case.to_string();
    //     if test_filter.contains_key(&case) {
    //         test_filter.remove(&case);
    //     }
    // }
    // drop(test_filter);
    exit_current_task(exit_code);
    Ok(0)
}
