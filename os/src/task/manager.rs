//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take the runnable process with the smallest stride.
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        if self.ready_queue.is_empty() {
            return None;
        }

        let mut min_index = 0;
        let mut min_stride =
            self.ready_queue[0].inner_exclusive_access().stride;

        for index in 1..self.ready_queue.len() {
            let candidate_stride =
                self.ready_queue[index].inner_exclusive_access().stride;

            if (candidate_stride.wrapping_sub(min_stride) as i64) < 0 {
                min_index = index;
                min_stride = candidate_stride;
            }
        }

        let task = self.ready_queue.remove(min_index).unwrap();
        {
            const BIG_STRIDE: u64 = 1 << 20;
            let mut inner = task.inner_exclusive_access();
            let pass = (BIG_STRIDE / inner.priority as u64).max(1);
            inner.stride = inner.stride.wrapping_add(pass);
        }
        Some(task)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
