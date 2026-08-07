use parking_lot::RwLock;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct LFRUCache<K, V> {
    capacity: usize,
    timestamp: AtomicU64,
    map: HashMap<K, Entry<V>>,
    evicted: Vec<V>,
}

struct Entry<V> {
    value: V,
    frequency: AtomicU64,
    last_access: AtomicU64,
}

impl<K: Eq + Hash + Clone, V> LFRUCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        LFRUCache {
            capacity,
            timestamp: AtomicU64::new(0),
            map: HashMap::with_capacity(capacity),
            evicted: Vec::new(),
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key).map(|entry| {
            let timestamp = self.timestamp.fetch_add(1, Ordering::Relaxed) + 1;
            entry.frequency.fetch_add(1, Ordering::Relaxed);
            entry.last_access.store(timestamp, Ordering::Relaxed);
            &entry.value
        })
    }

    pub fn put(&mut self, key: K, value: V) {
        if self.map.contains_key(&key) {
            if let Some(entry) = self.map.get_mut(&key) {
                let timestamp = self.timestamp.fetch_add(1, Ordering::Relaxed) + 1;
                entry.value = value;
                entry.frequency.fetch_add(1, Ordering::Relaxed);
                entry.last_access.store(timestamp, Ordering::Relaxed);
            }
            return;
        }
        if self.map.len() >= self.capacity {
            self.evict();
        }
        let ts = self.timestamp.fetch_add(1, Ordering::Relaxed) + 1;
        self.map.insert(
            key,
            Entry {
                value,
                frequency: AtomicU64::new(1),
                last_access: AtomicU64::new(ts),
            },
        );
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.map.remove(key).map(|e| e.value)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn take_evicted(&mut self) -> Vec<V> {
        std::mem::take(&mut self.evicted)
    }

    fn evict(&mut self) {
        if let Some(victim) = self
            .map
            .iter()
            .min_by_key(|(_k, e)| {
                (
                    e.frequency.load(Ordering::Relaxed),
                    e.last_access.load(Ordering::Relaxed),
                )
            })
            .map(|(k, _)| k.clone())
        {
            if let Some(entry) = self.map.remove(&victim) {
                self.evicted.push(entry.value);
            }
        }
    }
}

pub struct SyncLFRUCache<K, V> {
    inner: Arc<RwLock<LFRUCache<K, V>>>,
}

impl<K: Eq + Hash + Clone, V> SyncLFRUCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        SyncLFRUCache {
            inner: Arc::new(RwLock::new(LFRUCache::new(capacity))),
        }
    }

    pub fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        self.inner.read().get(key).cloned()
    }

    pub fn put(&self, key: K, value: V) {
        self.inner.write().put(key, value);
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        self.inner.write().remove(key)
    }

    pub fn take_evicted(&self) -> Vec<V> {
        self.inner.write().take_evicted()
    }

    pub fn len(&self) -> usize {
        self.inner.read().len()
    }
}

impl<K, V> Clone for SyncLFRUCache<K, V> {
    fn clone(&self) -> Self {
        SyncLFRUCache {
            inner: Arc::clone(&self.inner),
        }
    }
}
