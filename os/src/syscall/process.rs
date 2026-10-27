//! Process management syscalls
//!
use alloc::sync::Arc;

use crate::{
    fs::{open_file, OpenFlags},
    mm::{checked_str, copy_to_user},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next,
    },
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = match checked_str(token, path) {
        Some(path) => path,
        None => return -1,
    };
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        if !crate::mm::MemorySet::valid_elf(&all_data) {
            return -1;
        }
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    //trace!("kernel: sys_waitpid");
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let exit_code = inner.children[idx].inner_exclusive_access().exit_code;
        if copy_to_user(
            inner.memory_set.token(),
            exit_code_ptr as *mut u8,
            &exit_code.to_ne_bytes(),
        )
        .is_none()
        {
            return -1;
        }
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// Write time through the user page table, including split-page TimeVal values.
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    let us = crate::timer::get_time_us();
    let mut bytes = [0; core::mem::size_of::<TimeVal>()];
    let word = core::mem::size_of::<usize>();
    bytes[..word].copy_from_slice(&(us / 1_000_000).to_ne_bytes());
    bytes[word..].copy_from_slice(&(us % 1_000_000).to_ne_bytes());
    if copy_to_user(current_user_token(), ts as *mut u8, &bytes).is_some() {
        0
    } else {
        -1
    }
}

pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .memory_set
        .mmap(start, len, prot)
}

pub fn sys_munmap(start: usize, len: usize) -> isize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .memory_set
        .munmap(start, len)
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// Load a fresh child address space directly from the filesystem.
pub fn sys_spawn(path: *const u8) -> isize {
    let path = match checked_str(current_user_token(), path) {
        Some(path) => path,
        None => return -1,
    };
    let inode = match open_file(&path, OpenFlags::RDONLY) {
        Some(inode) => inode,
        None => return -1,
    };
    let data = inode.read_all();
    if !crate::mm::MemorySet::valid_elf(&data) {
        return -1;
    }
    let parent = current_task().unwrap();
    let child = Arc::new(crate::task::TaskControlBlock::new(&data));
    {
        let mut inner = child.inner_exclusive_access();
        inner.parent = Some(Arc::downgrade(&parent));
        inner.stride = parent.inner_exclusive_access().stride;
    }
    let pid = child.getpid();
    parent.inner_exclusive_access().children.push(child.clone());
    add_task(child);
    pid as isize
}

pub fn sys_set_priority(prio: isize) -> isize {
    if prio < 2 {
        return -1;
    }
    current_task().unwrap().inner_exclusive_access().priority = prio as usize;
    prio
}
