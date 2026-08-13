use crate::domain::models::cookie_models::{Cookie, CookieError, CookieKey};
use crate::domain::models::storage_models::WriteMode;
use crate::domain::traits::cookie_traits::CookieStore;
use crate::service::config::CookieConfig;
use crate::utils::url_component::extract_domain;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock as AsyncRwLock;
use tokio::time::timeout;
use tracing::{Instrument, Level, span};

pub struct FileBackedCookieStore {
    inner: AsyncRwLock<InnerStore>,
    config: CookieConfig,
    storage_path: Option<String>,
    dirty: std::sync::atomic::AtomicBool,
    _session: span::Span,
}

struct InnerStore {
    cookies: HashMap<CookieKey, Cookie>,
    session_cookies: HashMap<CookieKey, Cookie>,
    _session: span::Span,
}

#[async_trait]
impl CookieStore for FileBackedCookieStore {
    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn get(&self, key: &CookieKey) -> Option<Cookie> {
        let store = self.inner.read().await;

        if let Some(cookie) = store.cookies.get(key) {
            if !cookie.is_expired() {
                return Some(cookie.clone());
            }
        }

        store.session_cookies.get(key).cloned()
    }

    #[tracing::instrument(skip(self, cookie), parent = &self._session)]
    async fn set(&self, cookie: Cookie) {
        let mut store = self.inner.write().await;

        if cookie.persistent {
            store.cookies.insert(cookie.key.clone(), cookie);
        } else {
            store.session_cookies.insert(cookie.key.clone(), cookie);
        }

        self.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn remove(&self, key: &CookieKey) {
        let mut store = self.inner.write().await;
        store.cookies.remove(key);
        store.session_cookies.remove(key);
        self.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
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

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn get_for_url(&self, url: &str) -> Vec<Cookie> {
        let domain = extract_domain(url);
        if domain.is_err() {
            return vec![];
        }

        self.get_for_domain(&domain.unwrap()).await
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn clear_all(&self) {
        let mut store = self.inner.write().await;
        store.cookies.clear();
        store.session_cookies.clear();
        self.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn persist(&self) -> Result<(), CookieError> {
        if let Some(path) = &self.storage_path {
            tracing::debug!(path = ?path, "persisting");

            let store = self.inner.read().await;
            let serializable = SerializableStore {
                cookies: store.cookies.values().cloned().collect(),
                saved_at: SystemTime::now(),
            };

            let json = serde_json::to_string_pretty(&serializable).map_err(|e| {
                tracing::error!(error = %e, "serialize store to json error");
                CookieError::Serialization(e.to_string())
            })?;
            let content_bytes = json.into_bytes();
            tracing::debug!(content_length = ?content_bytes.len(), "writing json");

            let mut file = OpenOptions::new()
                .create(true)
                .append(false)
                .write(true)
                .open(path)
                .await
                .map_err(|e| {
                    tracing::debug!(file = ?path, error = %e, "open file error");
                    CookieError::IO(e.to_string())
                })?;
            match timeout(Duration::from_secs(60), file.write_all(&content_bytes)).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "async write error, downgrade to sync write");
                    std::fs::write(path, &content_bytes).map_err(|e| {
                        tracing::error!(error = %e, "sync write error");
                        CookieError::IO(e.to_string())
                    })
                }
                Err(e) => {
                    tracing::error!(error = %e, "async write timeout, downgrade to sync write");
                    std::fs::write(path, &content_bytes).map_err(|e| {
                        tracing::error!(error = %e, "sync write error");
                        CookieError::IO(e.to_string())
                    })
                }
            }
        } else {
            Ok(())
        }
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn load(&self) -> Result<(), CookieError> {
        if let Some(path) = &self.storage_path {
            tracing::debug!(path = ?path, "loading cookies");
            if !std::path::Path::new(path).exists() {
                tracing::debug!(path = ?path, "path not exists");
                return Ok(());
            }

            let json = tokio::fs::read_to_string(path).await.map_err(|e| {
                tracing::error!(error = %e, "read file error");
                CookieError::IO(e.to_string())
            })?;

            let serializable: SerializableStore = serde_json::from_str(&json).map_err(|e| {
                tracing::error!(error = %e, "deserialize error");
                CookieError::Serialization(e.to_string())
            })?;

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
        let backend_session = span!(Level::INFO, "cookie-store");
        let _ = backend_session.enter();
        tracing::debug!(
            cookie_path = ?config.cookie_path,
            debounce_delay_ms = ?config.debounce_delay.as_millis(),
            auto_save_interval_ms = ?config.auto_save_interval.map(|duration| duration.as_millis()),
            "creating cookie store"
        );

        let mut initial_cookies: HashMap<CookieKey, Cookie> = HashMap::new();
        if let Some(initials) = config.initial_cookies.clone() {
            tracing::debug!(cookie_count = ?initials.len(), "inserting initial cookies");
            initials.into_iter().for_each(|cookie| {
                let key = cookie.key.clone();
                initial_cookies.insert(key, cookie);
            });
        }

        let inner_session = span!(parent: &backend_session, Level::INFO, "inner-store");
        let store = Self {
            inner: AsyncRwLock::new(InnerStore {
                cookies: initial_cookies,
                session_cookies: HashMap::new(),
                _session: inner_session,
            }),
            storage_path: config.cookie_path.clone(),
            config,
            dirty: std::sync::atomic::AtomicBool::new(false),
            _session: backend_session,
        };

        store.load().await?;
        Ok(store)
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    pub fn start_auto_save(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        if let Some(interval) = self.config.auto_save_interval {
            tracing::debug!(interval_ms = ?interval.as_millis(), "spawning auto save thread");

            let store = Arc::clone(&self);
            tokio::spawn(
                async move {
                    let mut interval = tokio::time::interval(interval);
                    loop {
                        interval.tick().await;
                        if store.dirty.load(std::sync::atomic::Ordering::SeqCst) {
                            if let Err(e) = store.persist().await {
                                tracing::error!(error = %e, "Failed to auto-save cookies");
                            }
                        }
                    }
                }
                .in_current_span(),
            )
        } else {
            tokio::spawn(async {})
        }
    }
}
