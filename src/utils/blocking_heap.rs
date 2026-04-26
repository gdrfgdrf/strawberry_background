use parking_lot::{Condvar, Mutex};
use std::collections::BinaryHeap;
use std::time::Duration;

pub struct BlockingHeap<T: Ord> {
    inner: Mutex<BinaryHeap<T>>,
    capacity: usize,
    not_full: Condvar,
    not_empty: Condvar,
}

impl<T: Ord> BlockingHeap<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(BinaryHeap::with_capacity(capacity)),
            capacity,
            not_full: Condvar::new(),
            not_empty: Condvar::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn push(&self, item: T, timeout: Duration) -> Result<(), T> {
        let mut heap = self.inner.lock();
        while heap.len() >= self.capacity {
            let wait_result = self.not_full.wait_for(&mut heap, timeout);
            if wait_result.timed_out() {
                return Err(item);
            }
        }
        heap.push(item);
        self.not_empty.notify_one();
        Ok(())
    }

    pub fn pop(&self, timeout: Duration) -> Option<T> {
        let mut heap = self.inner.lock();
        loop {
            if let Some(item) = heap.pop() {
                self.not_full.notify_one();
                return Some(item);
            }
            let wait_result = self.not_empty.wait_for(&mut heap, timeout);
            if wait_result.timed_out() {
                return None;
            }
        }
    }
}
