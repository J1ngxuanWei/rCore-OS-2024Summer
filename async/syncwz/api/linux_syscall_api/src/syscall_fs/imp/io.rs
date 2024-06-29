//! 负责与 IO 相关的系统调用
extern crate alloc;
use crate::{SyscallError, SyscallResult};
use alloc::string::ToString;
use alloc::sync::Arc;
use axerrno::AxError;
use axfs::api::{FileIOType, OpenFlags};

use axlog::{debug, info};
use axtask::current_task;
use axtask::link::{create_link, deal_with_path};

use crate::syscall_fs::ctype::{
    dir::new_dir,
    file::{new_fd, new_inode},
};
/// 功能:从一个文件描述符中读取；
/// # Arguments
/// * `fd`: usize, 要读取文件的文件描述符。
/// * `buf`: *mut u8, 一个缓存区,用于存放读取的内容。
/// * `count`: usize, 要读取的字节数。
/// 返回值:成功执行,返回读取的字节数。如为0,表示文件结束。错误,则返回-1。
pub fn syscall_read(args: [usize; 6]) -> SyscallResult {
    let fd = args[0];
    let buf = args[1] as *mut u8;
    let count = args[2];
    info!("[read()] fd: {fd}, buf: {buf:?}, len: {count}",);

    if buf.is_null() {
        return Err(SyscallError::EFAULT);
    }

    let task = current_task();

    // TODO: 左闭右开
    let buf = match task.manual_alloc_range_for_lazy(
        (buf as usize).into(),
        (unsafe { buf.add(count) as usize } - 1).into(),
    ) {
        Ok(_) => unsafe { core::slice::from_raw_parts_mut(buf, count) },
        Err(_) => return Err(SyscallError::EFAULT),
    };

    let file = match task.fd_manager.fd_table.lock().get(fd) {
        Some(Some(f)) => f.clone(),
        _ => return Err(SyscallError::EBADF),
    };

    if file.get_type() == FileIOType::DirDesc {
        axlog::error!("fd is a dir");
        return Err(SyscallError::EISDIR);
    }

    // for sockets:
    // Sockets are "readable" when:
    // - have some data to read without blocking
    // - remote end send FIN packet, local read half is closed (this will return 0 immediately)
    //   this will return Ok(0)
    // - ready to accept new connections

    match file.read(buf) {
        Ok(len) => Ok(len as isize),
        Err(AxError::WouldBlock) => Err(SyscallError::EAGAIN),
        Err(AxError::InvalidInput) => Err(SyscallError::EINVAL),
        Err(_) => Err(SyscallError::EPERM),
    }
}

/// 功能:从一个文件描述符中写入；
/// # Arguments:
/// * `fd`: usize, 要写入文件的文件描述符。
/// * `buf`: *const u8, 一个缓存区,用于存放要写入的内容。
/// * `count`: usize, 要写入的字节数。
/// 返回值:成功执行,返回写入的字节数。错误,则返回-1。
pub fn syscall_write(args: [usize; 6]) -> SyscallResult {
    let fd = args[0];
    let buf = args[1] as *const u8;
    let count = args[2];
    if buf.is_null() {
        return Err(SyscallError::EFAULT);
    }

    let task = current_task();

    // TODO: 左闭右开
    let buf = match task.manual_alloc_range_for_lazy(
        (buf as usize).into(),
        (unsafe { buf.add(count) as usize } - 1).into(),
    ) {
        Ok(_) => unsafe { core::slice::from_raw_parts(buf, count) },
        Err(_) => return Err(SyscallError::EFAULT),
    };

    let file = match task.fd_manager.fd_table.lock().get(fd) {
        Some(Some(f)) => f.clone(),
        _ => return Err(SyscallError::EBADF),
    };

    if file.get_type() == FileIOType::DirDesc {
        debug!("fd is a dir");
        return Err(SyscallError::EBADF);
    }

    // for sockets:
    // Sockets are "writable" when:
    // - connected and have space in tx buffer to write
    // - sent FIN packet, local send half is closed (this will return 0 immediately)
    //   this will return Err(ConnectionReset)

    match file.write(buf) {
        Ok(len) => Ok(len as isize),
        // socket with send half closed
        // TODO: send a SIGPIPE signal to the task
        Err(axerrno::AxError::ConnectionReset) => Err(SyscallError::EPIPE),
        Err(AxError::WouldBlock) => Err(SyscallError::EAGAIN),
        Err(AxError::InvalidInput) => Err(SyscallError::EINVAL),
        Err(_) => Err(SyscallError::EPERM),
    }
}


/// 功能:打开或创建一个文件；
/// # Arguments
/// * `fd`: usize, 文件所在目录的文件描述符。
/// * `path`: *const u8, 要打开或创建的文件名。如为绝对路径,则忽略fd。如为相对路径,且fd是AT_FDCWD,则filename是相对于当前工作目录来说的。如为相对路径,且fd是一个文件描述符,则filename是相对于fd所指向的目录来说的。
/// * `flags`: usize, 必须包含如下访问模式的其中一种:O_RDONLY,O_WRONLY,O_RDWR。还可以包含文件创建标志和文件状态标志。
/// * `mode`: u8, 文件的所有权描述。详见`man 7 inode `。
/// 返回值:成功执行,返回新的文件描述符。失败,返回-1。
///
/// 说明:如果打开的是一个目录,那么返回的文件描述符指向的是该目录的描述符。(后面会用到针对目录的文件描述符)
/// flags: O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 64, O_DIRECTORY: 65536
pub fn syscall_openat(args: [usize; 6]) -> SyscallResult {
    let fd = args[0];
    let path = args[1] as *const u8;
    let flags = args[2];
    let _mode = args[3] as u8;
    let force_dir = OpenFlags::from(flags).is_dir();
    let path = if let Some(path) = deal_with_path(fd, Some(path), force_dir) {
        path
    } else {
        return Err(SyscallError::EINVAL);
    };
    let task = current_task();
    let mut fd_table = task.fd_manager.fd_table.lock();
    let fd_num: usize = if let Ok(fd) = task.alloc_fd(&mut fd_table) {
        fd
    } else {
        return Err(SyscallError::EMFILE);
    };
    debug!("allocated fd_num: {}", fd_num);
    // 分配 inode
    new_inode(path.path().to_string()).unwrap();
    // 如果是DIR
    info!("path: {:?}", path.path());
    if path.is_dir() {
        debug!("open dir");
        if let Ok(dir) = new_dir(path.path().to_string(), flags.into()) {
            debug!("new dir_desc successfully allocated: {}", path.path());
            fd_table[fd_num] = Some(Arc::new(dir));
            Ok(fd_num as isize)
        } else {
            debug!("open dir failed");
            Err(SyscallError::ENOENT)
        }
    }
    // 如果是FILE,注意若创建了新文件,需要添加链接
    else {
        debug!("open file");
        if let Ok(file) = new_fd(path.path().to_string(), flags.into()) {
            debug!("new file_desc successfully allocated");
            fd_table[fd_num] = Some(Arc::new(file));
            let _ = create_link(&path, &path); // 不需要检查是否成功,因为如果成功,说明是新建的文件,如果失败,说明已经存在了
            Ok(fd_num as isize)
        } else {
            debug!("open file failed");
            Err(SyscallError::ENOENT)
        }
    }
}



/// 功能:关闭一个文件描述符；
/// # Arguments
/// * `fd`: usize, 要关闭的文件描述符。
/// 返回值:成功执行,返回0。失败,返回-1。
pub fn syscall_close(args: [usize; 6]) -> SyscallResult {
    let fd = args[0];
    info!("Into syscall_close. fd: {}", fd);

    let task = current_task();
    let mut fd_table = task.fd_manager.fd_table.lock();
    if fd >= fd_table.len() {
        debug!("fd {} is out of range", fd);
        return Err(SyscallError::EPERM);
    }

    if fd_table[fd].is_none() {
        debug!("fd {} is none", fd);
        return Err(SyscallError::EPERM);
    }

    fd_table[fd] = None;

    Ok(0)
}

