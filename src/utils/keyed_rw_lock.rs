use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use tokio::sync::{RwLock, RwLockReadGuard};

pub struct KeyedRwLock<T> {
    cumulative_cleanup: AtomicI32,
    locks: DashMap<String, Arc<RwLock<T>>>,
}

impl<T> KeyedRwLock<T> {
    pub fn new() -> Self {
        Self {
            cumulative_cleanup: AtomicI32::new(0),
            locks: DashMap::new(),
        }
    }

    pub async fn read<F, R>(&self, id: &str, operation: F) -> R
    where
        F: FnOnce(&T) -> R,
        T: Default,
    {
        self.cumulate_cleanup();

        let lock = self
            .locks
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(T::default())));
        let guard = lock.value().read().await;
        operation(&guard)
    }

    pub async fn write<F, R>(&self, id: &str, operation: F) -> R
    where
        F: FnOnce(&mut T) -> R,
        T: Default,
    {
        self.cumulate_cleanup();

        let lock = self
            .locks
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(T::default())));
        let mut guard = lock.value().write().await;
        operation(&mut guard)
    }

    pub fn free(&self, id: &str) -> Option<(String, T)> {
        if !self.locks.contains_key(id) {
            return None;
        }

        let lock = self.locks.remove(id)?;

        let key = lock.0;
        let rwlock = Arc::into_inner(lock.1)?;
        let value = rwlock.into_inner();

        Some((key, value))
    }

    pub fn cleanup(&self) {
        self.locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        self.cumulative_cleanup.store(0, Ordering::SeqCst);
    }

    fn cumulate_cleanup(&self) {
        let target = self.cumulative_cleanup.fetch_add(1, Ordering::SeqCst) + 1;
        if target >= 32 {
            self.cleanup();
        }
    }
}
