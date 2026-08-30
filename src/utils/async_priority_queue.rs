use std::collections::BinaryHeap;
use tokio::sync::{Mutex, Notify};

pub struct AsyncPriorityQueue<T: Ord> {
    heap: Mutex<BinaryHeap<T>>,
    notifier: Notify,
}

impl<T: Ord> AsyncPriorityQueue<T> {
    pub fn unbounded() -> Self {
        Self {
            heap: Mutex::new(BinaryHeap::new()),
            notifier: Notify::new(),
        }
    }
    
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            heap: Mutex::new(BinaryHeap::with_capacity(capacity)),
            notifier: Notify::new(),
        }
    }

    pub async fn push(&self, item: T) {
        let mut heap = self.heap.lock().await;
        heap.push(item);
        self.notifier.notify_one();
    }

    pub async fn pop(&self) -> T {
        loop {
            let mut heap = self.heap.lock().await;
            if let Some(item) = heap.pop() {
                return item;
            }
            drop(heap);
            self.notifier.notified().await;
        }
    }
}