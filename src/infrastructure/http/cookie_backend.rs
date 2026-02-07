use crate::domain::models::cookie_models::{Cookie, CookieError, CookieKey};
use crate::domain::traits::cookie_traits::CookieStore;
use crate::service::config::CookieConfig;
use crate::utils::url_component::extract_domain;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock as AsyncRwLock;
use tokio::time::timeout;

pub struct FileBackedCookieStore {
    inner: AsyncRwLock<InnerStore>,
    config: CookieConfig,
    storage_path: Option<String>,
    dirty: std::sync::atomic::AtomicBool,
}

struct InnerStore {
    cookies: HashMap<CookieKey, Cookie>,
    session_cookies: HashMap<CookieKey, Cookie>,
}

#[async_trait]
impl CookieStore for FileBackedCookieStore {
    async fn get(&self, key: &CookieKey) -> Option<Cookie> {
        let store = self.inner.read().await;

        if let Some(cookie) = store.cookies.get(key) {
            if !cookie.is_expired() {
                return Some(cookie.clone());
            }
        }

        store.session_cookies.get(key).cloned()
    }

    async fn set(&self, cookie: Cookie) {
        let mut store = self.inner.write().await;

        if cookie.persistent {
            store.cookies.insert(cookie.key.clone(), cookie);
        } else {
            store.session_cookies.insert(cookie.key.clone(), cookie);
        }

        self.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    async fn remove(&self, key: &CookieKey) {
        let mut store = self.inner.write().await;
        store.cookies.remove(key);
        store.session_cookies.remove(key);
        self.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    async fn get_for_domain(&self, domain: &str) -> Vec<Cookie> {
        let store = self.inner.read().await;

        let mut cookies = Vec::new();
        let now = SystemTime::now();

        for cookie in store.cookies.values() {
            if cookie.key.domain == domain {
                match cookie.expires {
                    Some(expires) if expires < now => continue,
                    _ => cookies.push(cookie.clone()),
                }
            }
        }

        for cookie in store.session_cookies.values() {
            if cookie.key.domain == domain {
                cookies.push(cookie.clone());
            }
        }

        cookies
    }

    async fn get_for_url(&self, url: &str) -> Vec<Cookie> {
        let domain = extract_domain(url);
        if domain.is_err() {
            return vec![];
        }

        self.get_for_domain(&domain.unwrap()).await
    }

    async fn clear_all(&self) {
        let mut store = self.inner.write().await;
        store.cookies.clear();
        store.session_cookies.clear();
        self.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    async fn persist(&self) -> Result<(), CookieError> {
        if let Some(path) = &self.storage_path {
            let store = self.inner.read().await;
            let serializable = SerializableStore {
                cookies: store.cookies.values().cloned().collect(),
                saved_at: SystemTime::now(),
            };

            let json = serde_json::to_string_pretty(&serializable)
                .map_err(|e| CookieError::Serialization(e.to_string()))?;
            match timeout(
                Duration::from_secs(60),
                tokio::fs::write(path, json.into_bytes()),
            )
            .await
            {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(CookieError::IO(e.to_string())),
                Err(e) => Err(CookieError::Timeout(e.to_string())),
            }
        } else {
            Ok(())
        }
    }

    async fn load(&self) -> Result<(), CookieError> {
        if let Some(path) = &self.storage_path {
            if !std::path::Path::new(path).exists() {
                return Ok(());
            }

            let json = tokio::fs::read_to_string(path)
                .await
                .map_err(|e| CookieError::IO(e.to_string()))?;

            let serializable: SerializableStore = serde_json::from_str(&json)
                .map_err(|e| CookieError::Serialization(e.to_string()))?;

            let now = SystemTime::now();
            let cookies: HashMap<_, _> = serializable
                .cookies
                .into_iter()
                .filter(|cookie| match cookie.expires {
                    Some(expires) => expires > now,
                    None => true,
                })
                .map(|cookie| (cookie.key.clone(), cookie))
                .collect();

            let mut store = self.inner.write().await;
            store.cookies = cookies;

            Ok(())
        } else {
            Ok(())
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SerializableStore {
    cookies: Vec<Cookie>,
    saved_at: SystemTime,
}

impl FileBackedCookieStore {
    pub async fn new(config: CookieConfig) -> Result<Self, CookieError> {
        let mut initial_cookies: HashMap<CookieKey, Cookie> = HashMap::new();
        if let Some(initials) = config.initial_cookies.clone() {
            initials.into_iter().for_each(|cookie| {
                let key = cookie.key.clone();
                initial_cookies.insert(key, cookie);
            });
        }

        let store = Self {
            inner: AsyncRwLock::new(InnerStore {
                cookies: initial_cookies,
                session_cookies: HashMap::new(),
            }),
            storage_path: config.cookie_path.clone(),
            config,
            dirty: std::sync::atomic::AtomicBool::new(false),
        };

        store.load().await?;
        Ok(store)
    }

    pub fn start_auto_save(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        if let Some(interval) = self.config.auto_save_interval {
            let store = Arc::clone(&self);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(interval);
                loop {
                    interval.tick().await;
                    if store.dirty.load(std::sync::atomic::Ordering::SeqCst) {
                        if let Err(e) = store.persist().await {
                            eprintln!("Failed to auto-save cookies: {}", e);
                        }
                    }
                }
            })
        } else {
            tokio::spawn(async {})
        }
    }
}
