//! File and filesystem-related syscalls
use crate::fs::{link_file, open_file, unlink_file, OpenFlags, Stat};
use crate::mm::{checked_byte_buffer, checked_str, copy_to_user, UserBuffer};
use crate::task::{current_task, current_user_token};

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        match checked_byte_buffer(token, buf, len, false) {
            Some(buffers) => file.write(UserBuffer::new(buffers)) as isize,
            None => -1,
        }
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        match checked_byte_buffer(token, buf, len, true) {
            Some(buffers) => file.read(UserBuffer::new(buffers)) as isize,
            None => -1,
        }
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = match checked_str(token, path) {
        Some(path) => path,
        None => return -1,
    };
    let flags = match OpenFlags::from_bits(flags) {
        Some(flags) => flags,
        None => return -1,
    };
    if let Some(inode) = open_file(path.as_str(), flags) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    trace!("kernel:pid[{}] sys_fstat", current_task().unwrap().pid.0);

    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();

    if fd >= inner.fd_table.len() {
        return -1;
    }

    let file = match &inner.fd_table[fd] {
        Some(file) => file.clone(),
        None => return -1,
    };
    drop(inner);

    let stat = match file.stat() {
        Some(stat) => stat,
        None => return -1,
    };

    let bytes = unsafe {
        core::slice::from_raw_parts(
            &stat as *const Stat as *const u8,
            core::mem::size_of::<Stat>(),
        )
    };

    if copy_to_user(token, st as *mut u8, bytes).is_some() {
        0
    } else {
        -1
    }
}

pub fn sys_linkat(old_name: *const u8, new_name: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_linkat", current_task().unwrap().pid.0);

    let token = current_user_token();
    let old_name = match checked_str(token, old_name) {
        Some(name) => name,
        None => return -1,
    };
    let new_name = match checked_str(token, new_name) {
        Some(name) => name,
        None => return -1,
    };

    if link_file(old_name.as_str(), new_name.as_str()) {
        0
    } else {
        -1
    }
}

pub fn sys_unlinkat(name: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_unlinkat", current_task().unwrap().pid.0);

    let name = match checked_str(current_user_token(), name) {
        Some(name) => name,
        None => return -1,
    };
    if unlink_file(name.as_str()) {
        0
    } else {
        -1
    }
}
