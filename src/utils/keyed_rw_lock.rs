use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct KeyedRwLock<T> {
    locks: DashMap<String, Arc<RwLock<T>>>,
}

impl<T> KeyedRwLock<T> {
    pub fn new() -> Self {
        Self {
            locks: DashMap::new(),
        }
    }

    pub async fn read<F, R>(&self, id: &str, operation: F) -> Option<R>
    where
        F: FnOnce(&T) -> R,
        T: Default,
    {
        let lock = self
            .locks
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(T::default())));
        let guard = lock.value().read().await;
        Some(operation(&guard))
    }

    pub async fn write<F, R>(&self, id: &str, operation: F) -> Option<R>
    where
        F: FnOnce(&mut T) -> R,
        T: Default,
    {
        let lock = self
            .locks
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(T::default())));
        let mut guard = lock.value().write().await;
        Some(operation(&mut guard))
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
    }
}
