use crate::db::models::preclude::{
    CookieKeysActiveModel, CookieKeysModel, CookiesActiveModel, CookiesModel,
};
use crate::db::services::cookie_service::CookieService;
use crate::domain::models::cookie_models::CookieError;
use crate::domain::traits::cookie_traits::CookieStore;
use crate::service::config::CookieConfig;
use crate::utils::url_component::extract_domain;
use chrono::Utc;
use parking_lot::RwLock;
use sea_orm::{ActiveValue, TryIntoModel};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use tracing::{Level, span};

pub struct DatabaseCookieStore {
    inner: RwLock<InnerStore>,
    _session: span::Span,
}

struct InnerStore {
    session_cookies: HashMap<CookieKeysModel, CookiesModel>,
    session_cookie_id: AtomicI64,
    _session: span::Span,
}

impl CookieStore for DatabaseCookieStore {
    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn get(&self, key: &CookieKeysModel) -> Result<Option<CookiesModel>, CookieError> {
        let value = CookieService::find_value_by_key_id(key.id).await?;
        if value.is_some() {
            return Ok(value);
        }

        let store = self.inner.read();
        Ok(store.session_cookies.get(key).cloned())
    }

    #[tracing::instrument(skip(self, cookie), parent = &self._session)]
    async fn set(
        &self,
        mut key: CookieKeysActiveModel,
        mut cookie: CookiesActiveModel,
        persistent: bool,
    ) -> Result<(), CookieError> {
        if persistent {
            CookieService::insert(key, cookie).await?;
            return Ok(());
        }

        let mut store = self.inner.write();
        let id = store.increase_and_get_id();
        key.id = ActiveValue::Set(id.clone());
        cookie.id = ActiveValue::Set(id.clone());
        cookie.key_id = ActiveValue::Set(id);
        let key = key
            .try_into_model()
            .map_err(|e| CookieError::ErrorForward(e.to_string()))?;
        let cookie = cookie
            .try_into_model()
            .map_err(|e| CookieError::ErrorForward(e.to_string()))?;

        store.session_cookies.insert(key, cookie);

        Ok(())
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn remove(&self, key: &CookieKeysModel) -> Result<(), CookieError> {
        let affected = CookieService::remove_key_by_key_id(key.id).await?;
        if affected {
            return Ok(());
        }

        let mut store = self.inner.write();
        store.session_cookies.remove(key);

        Ok(())
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn get_for_domain(
        &self,
        domain: &str,
    ) -> Result<Vec<(CookieKeysModel, CookiesModel)>, CookieError> {
        let pairs = CookieService::find_by_domain(domain.to_string()).await?;
        let mut pairs = pairs
            .into_iter()
            .filter(|(_, value)| !Self::is_expired(value))
            .collect::<Vec<(CookieKeysModel, CookiesModel)>>();

        let store = self.inner.read();
        for (key, value) in store.session_cookies.iter() {
            if key.domain == domain {
                pairs.push((key.clone(), value.clone()));
            }
        }

        Ok(pairs)
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn get_for_url(
        &self,
        url: &str,
    ) -> Result<Vec<(CookieKeysModel, CookiesModel)>, CookieError> {
        let domain = extract_domain(url);
        if domain.is_err() {
            return Ok(vec![]);
        }

        self.get_for_domain(&domain.unwrap()).await
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn clear_all(&self) -> Result<(), CookieError> {
        CookieService::clear_all().await?;

        let mut store = self.inner.write();
        store.session_cookies.clear();

        Ok(())
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn persist(&self) -> Result<(), CookieError> {
        Ok(())
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn load(&self) -> Result<(), CookieError> {
        Ok(())
    }
}

impl DatabaseCookieStore {
    pub async fn new(config: CookieConfig) -> Result<Self, CookieError> {
        let backend_session = span!(Level::INFO, "cookie-store");
        let _ = backend_session.enter();

        for (key, value) in config.initial_cookies.unwrap_or(vec![]).into_iter() {
            CookieService::insert(key, value).await?;
        }

        let inner_session = span!(parent: &backend_session, Level::INFO, "inner-store");
        let store = Self {
            inner: RwLock::new(InnerStore {
                session_cookies: HashMap::new(),
                session_cookie_id: AtomicI64::new(0),
                _session: inner_session,
            }),
            _session: backend_session,
        };

        store.load().await?;
        Ok(store)
    }

    pub fn is_expired(value: &CookiesModel) -> bool {
        match value.expires_at {
            Some(expires_at) => Utc::now() > expires_at.with_timezone(&Utc),
            None => false,
        }
    }
}

impl InnerStore {
    fn increase_and_get_id(&self) -> i64 {
        self.session_cookie_id.fetch_add(1, Ordering::SeqCst)
    }
}
