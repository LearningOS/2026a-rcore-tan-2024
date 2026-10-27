//! Semaphore

use crate::sync::UPSafeCell;
use crate::task::{
    block_current_and_run_next, current_process, current_task, current_tid, wakeup_task,
    TaskControlBlock,
};
use alloc::{collections::VecDeque, sync::Arc};

/// semaphore structure
pub struct Semaphore {
    id: usize,
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(id: usize, res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            id,
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self) {
        trace!("kernel: Semaphore::up");
        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        process_inner
            .semaphore_resources
            .release(current_tid(), self.id);
        let mut inner = self.inner.exclusive_access();
        inner.count += 1;
        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                let tid = task.inner_exclusive_access().res.as_ref().unwrap().tid;
                process_inner.semaphore_resources.acquire(tid, self.id);
                wakeup_task(task);
            }
        }
    }

    /// down operation of semaphore
    pub fn down(&self) -> isize {
        trace!("kernel: Semaphore::down");
        let process = current_process();
        let tid = current_tid();
        let mut process_inner = process.inner_exclusive_access();
        let enabled = process_inner.deadlock_detect_enabled;
        if !process_inner
            .semaphore_resources
            .request(tid, self.id, enabled)
        {
            return -0xDEAD;
        }
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            drop(process_inner);
            drop(process);
            block_current_and_run_next();
        } else {
            process_inner.semaphore_resources.acquire(tid, self.id);
        }
        0
    }
}
