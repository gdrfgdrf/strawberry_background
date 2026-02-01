use std::time::SystemTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct CookieKey {
    pub domain: String,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cookie {
    pub key: CookieKey,
    pub value: String,
    pub expires: Option<SystemTime>,
    pub creation_time: SystemTime,
    pub last_access_time: SystemTime,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<SameSite>,
    pub persistent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(PartialEq)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

#[derive(Debug, thiserror::Error)]
pub enum CookieError {
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("IO error: {0}")]
    Io(String),
}

impl Cookie {
    pub fn new(
        domain: String,
        path: String,
        name: String,
        value: String,
        expires: Option<SystemTime>,
        secure: bool,
        http_only: bool,
        same_site: Option<SameSite>,
    ) -> Self {
        let now = SystemTime::now();
        Self {
            key: CookieKey { domain, path, name },
            value,
            expires,
            creation_time: now,
            last_access_time: now,
            secure,
            http_only,
            same_site,
            persistent: expires.is_some(),
        }
    }
    
    pub fn new_without_expires(
        domain: String,
        path: String,
        name: String,
        value: String,
        secure: bool,
        http_only: bool,
        same_site: Option<SameSite>,
    ) -> Self {
        let now = SystemTime::now();
        Self {
            key: CookieKey { domain, path, name },
            value,
            expires: None,
            creation_time: now,
            last_access_time: now,
            secure,
            http_only,
            same_site,
            persistent: false,
        }
    }

    pub fn is_expired(&self) -> bool {
        match self.expires {
            Some(expires) => SystemTime::now() > expires,
            None => false,
        }
    }

    pub fn matches_url(&self, url: &str) -> bool {
        url.contains(&self.key.domain)
    }
}