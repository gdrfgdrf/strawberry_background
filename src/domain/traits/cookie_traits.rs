use async_trait::async_trait;
use crate::domain::models::cookie_models::{Cookie, CookieError, CookieKey};

#[async_trait]
pub trait CookieStore: Send + Sync + 'static {
    async fn get(&self, key: &CookieKey) -> Option<Cookie>;

    async fn set(&self, cookie: Cookie);

    async fn remove(&self, key: &CookieKey);

    async fn get_for_domain(&self, domain: &str) -> Vec<Cookie>;

    async fn get_for_url(&self, url: &str) -> Vec<Cookie>;

    async fn clear_all(&self);

    async fn persist(&self) -> Result<(), CookieError>;

    async fn load(&self) -> Result<(), CookieError>;
}
