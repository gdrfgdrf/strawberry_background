use std::any::Any;
use std::sync::Arc;
use crate::db::models::preclude::{CookieKeysActiveModel, CookieKeysModel, CookiesActiveModel, CookiesModel};
use crate::domain::models::cookie_models::CookieError;

pub trait CookieStore {
    async fn get(&self, key: &CookieKeysModel) -> Result<Option<CookiesModel>, CookieError>;

    async fn set(&self, key: CookieKeysActiveModel, cookie: CookiesActiveModel, persistent: bool) -> Result<(), CookieError>;

    async fn remove(&self, key: &CookieKeysModel) -> Result<(), CookieError>;

    async fn get_for_domain(&self, domain: &str) -> Result<Vec<(CookieKeysModel, CookiesModel)>, CookieError>;

    async fn get_for_url(&self, url: &str) -> Result<Vec<(CookieKeysModel, CookiesModel)>, CookieError>;

    async fn clear_all(&self) -> Result<(), CookieError>;

    async fn persist(&self) -> Result<(), CookieError>;

    async fn load(&self) -> Result<(), CookieError>;
}
