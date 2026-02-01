use std::any::Any;
use std::sync::Arc;
use async_trait::async_trait;
use crate::domain::models::cookie_models::{Cookie, CookieError, CookieKey};

impl dyn CookieStore {
    pub fn downcast_arc<T: CookieStore>(self: Arc<Self>) -> Option<Arc<T>> {
        let any_arc = self as Arc<dyn Any>;
        if any_arc.is::<T>() {
            let raw_ptr = Arc::into_raw(any_arc) as *const T;
            Some(unsafe { Arc::from_raw(raw_ptr) })
        } else {
            None
        }
    }
}

#[async_trait]
pub trait CookieStore: Any + Send + Sync + 'static {
    async fn get(&self, key: &CookieKey) -> Option<Cookie>;

    async fn set(&self, cookie: Cookie);

    async fn remove(&self, key: &CookieKey);

    async fn get_for_domain(&self, domain: &str) -> Vec<Cookie>;

    async fn get_for_url(&self, url: &str) -> Vec<Cookie>;

    async fn clear_all(&self);

    async fn persist(&self) -> Result<(), CookieError>;

    async fn load(&self) -> Result<(), CookieError>;
}
