//! Resource accounting and deadlock detection for one process and resource class.

use alloc::{vec, vec::Vec};

pub(crate) struct ResourceTracker {
    available: Vec<usize>,
    allocation: Vec<Vec<usize>>,
    need: Vec<Vec<usize>>,
}

impl ResourceTracker {
    pub(crate) fn new() -> Self {
        Self {
            available: Vec::new(),
            allocation: Vec::new(),
            need: Vec::new(),
        }
    }

    pub(crate) fn add_resource(&mut self, id: usize, count: usize) {
        self.available.resize(self.available.len().max(id + 1), 0);
        self.available[id] = count;
        for row in self.allocation.iter_mut().chain(self.need.iter_mut()) {
            row.resize(self.available.len(), 0);
            row[id] = 0;
        }
    }

    /// Record a pending request; reject it without changing any resource ownership.
    pub(crate) fn request(&mut self, tid: usize, id: usize, detect: bool) -> bool {
        while self.need.len() <= tid {
            self.need.push(vec![0; self.available.len()]);
            self.allocation.push(vec![0; self.available.len()]);
        }
        self.need[tid][id] += 1;
        if detect && !self.is_safe() {
            self.need[tid][id] -= 1;
            return false;
        }
        true
    }

    fn is_safe(&self) -> bool {
        let mut work = self.available.clone();
        let mut finish = vec![false; self.need.len()];
        loop {
            let mut progress = false;
            for tid in 0..finish.len() {
                if !finish[tid]
                    && self.need[tid]
                        .iter()
                        .zip(&work)
                        .all(|(need, free)| need <= free)
                {
                    for (free, allocated) in work.iter_mut().zip(&self.allocation[tid]) {
                        *free += allocated;
                    }
                    finish[tid] = true;
                    progress = true;
                }
            }
            if !progress {
                return finish.iter().all(|done| *done);
            }
        }
    }

    /// Complete a request at acquisition or direct handoff, before waking the waiter.
    pub(crate) fn acquire(&mut self, tid: usize, id: usize) {
        self.available[id] -= 1;
        self.need[tid][id] -= 1;
        self.allocation[tid][id] += 1;
    }

    pub(crate) fn owns(&self, tid: usize, id: usize) -> bool {
        self.allocation.get(tid).map_or(false, |row| row[id] > 0)
    }

    /// A semaphore producer may signal without having previously acquired a unit.
    pub(crate) fn release(&mut self, tid: usize, id: usize) {
        if self.owns(tid, id) {
            self.allocation[tid][id] -= 1;
        }
        self.available[id] += 1;
    }

    /// Exited threads cannot satisfy pending requests or release held resources.
    pub(crate) fn remove_thread(&mut self, tid: usize) {
        if tid < self.need.len() {
            self.need[tid].fill(0);
            self.allocation[tid].fill(0);
        }
    }
}
