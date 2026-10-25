//! Process management syscalls
use crate::mm::translated_byte_buffer;
use crate::task::{
    change_program_brk, current_user_token, exit_current_and_run_next,
    suspend_current_and_run_next,
};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");

    let time_us = get_time_us();
    let timeval = TimeVal {
        sec: time_us / 1_000_000,
        usec: time_us % 1_000_000,
    };

    let sec_bytes = timeval.sec.to_ne_bytes();
    let usec_bytes = timeval.usec.to_ne_bytes();
    let mut data = [0u8; core::mem::size_of::<TimeVal>()];

    data[..sec_bytes.len()].copy_from_slice(&sec_bytes);
    data[sec_bytes.len()..sec_bytes.len() + usec_bytes.len()]
        .copy_from_slice(&usec_bytes);

    let buffers =
        translated_byte_buffer(current_user_token(), ts as *const u8, data.len());

    let mut offset = 0;
    for buffer in buffers {
        let end = offset + buffer.len();
        buffer.copy_from_slice(&data[offset..end]);
        offset = end;
    }

    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");

    if trace_request == 2 {
        return crate::task::syscall_count(id) as isize;
    }
    if trace_request != 0 && trace_request != 1 {
        return -1;
    }

    if id >= (1usize << 38) {
        return -1;
    }

    let va = crate::mm::VirtAddr::from(id);
    let table = crate::mm::PageTable::from_token(current_user_token());

    let pte = match table.translate(va.floor()) {
        Some(pte) if pte.is_valid() => pte,
        _ => return -1,
    };

    if !pte.flags().contains(crate::mm::PTEFlags::U) {
        return -1;
    }

    if trace_request == 0 {
        if !pte.readable() {
            return -1;
        }
        pte.ppn().get_bytes_array()[va.page_offset()] as isize
    } else {
        if !pte.writable() {
            return -1;
        }
        pte.ppn().get_bytes_array()[va.page_offset()] = data as u8;
        0
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    crate::task::mmap_current(start, len, prot)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    crate::task::munmap_current(start, len)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
