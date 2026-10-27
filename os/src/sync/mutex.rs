//! Mutex (spin-like and blocking(sleep))

use super::UPSafeCell;
use crate::task::TaskControlBlock;
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_process, current_task, current_tid, wakeup_task};
use alloc::{collections::VecDeque, sync::Arc};

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self) -> isize;
    /// Unlock the mutex
    fn unlock(&self) -> isize;
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    id: usize,
    locked: UPSafeCell<bool>,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new(id: usize) -> Self {
        Self {
            id,
            locked: unsafe { UPSafeCell::new(false) },
        }
    }
}

impl Mutex for MutexSpin {
    /// Lock the spinlock mutex
    fn lock(&self) -> isize {
        trace!("kernel: MutexSpin::lock");
        let process = current_process();
        let tid = current_tid();
        {
            let mut inner = process.inner_exclusive_access();
            let enabled = inner.deadlock_detect_enabled;
            if !inner.mutex_resources.request(tid, self.id, enabled) {
                return -0xDEAD;
            }
        }
        drop(process);
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;
                current_process()
                    .inner_exclusive_access()
                    .mutex_resources
                    .acquire(tid, self.id);
                return 0;
            }
        }
    }

    fn unlock(&self) -> isize {
        trace!("kernel: MutexSpin::unlock");
        let process = current_process();
        let tid = current_tid();
        let mut inner = process.inner_exclusive_access();
        if !inner.mutex_resources.owns(tid, self.id) {
            return -1;
        }
        inner.mutex_resources.release(tid, self.id);
        let mut locked = self.locked.exclusive_access();
        *locked = false;
        0
    }
}

/// Blocking Mutex struct
pub struct MutexBlocking {
    id: usize,
    inner: UPSafeCell<MutexBlockingInner>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl MutexBlocking {
    /// Create a new blocking mutex
    pub fn new(id: usize) -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            id,
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }
}

impl Mutex for MutexBlocking {
    /// lock the blocking mutex
    fn lock(&self) -> isize {
        trace!("kernel: MutexBlocking::lock");
        let process = current_process();
        let tid = current_tid();
        let mut inner = process.inner_exclusive_access();
        let enabled = inner.deadlock_detect_enabled;
        if !inner.mutex_resources.request(tid, self.id, enabled) {
            return -0xDEAD;
        }
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            drop(inner);
            drop(process);
            block_current_and_run_next();
        } else {
            mutex_inner.locked = true;
            inner.mutex_resources.acquire(tid, self.id);
        }
        0
    }

    /// unlock the blocking mutex
    fn unlock(&self) -> isize {
        trace!("kernel: MutexBlocking::unlock");
        let process = current_process();
        let tid = current_tid();
        let mut inner = process.inner_exclusive_access();
        if !inner.mutex_resources.owns(tid, self.id) {
            return -1;
        }
        inner.mutex_resources.release(tid, self.id);
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            let tid = waking_task
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .tid;
            inner.mutex_resources.acquire(tid, self.id);
            wakeup_task(waking_task);
        } else {
            mutex_inner.locked = false;
        }
        0
    }
}
