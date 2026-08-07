use crate::domain::models::http_models::{
    HttpClientError, HttpEndpoint, HttpResponse, HttpStreamResponse,
};
use crate::domain::models::storage_models::{ReadFile, StorageError, WriteFile};
use crate::domain::traits::cookie_traits::CookieStore;
use crate::domain::traits::http_traits::HttpClient;
use crate::domain::traits::storage_traits::StorageManager;
use crate::infrastructure::http::cookie_backend::FileBackedCookieStore;
use crate::infrastructure::http::reqwest_backend::ReqwestBackend;
use crate::infrastructure::storage::storage_backend::AsyncStorageManager;
use crate::service::config::{
    CookieConfig, HttpConfig, RuntimeConfig,
};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("Tokio runtime initialization failed: {0}")]
    TokioInit(String),
    #[error("HTTP client initialization failed: {0}")]
    HttpClientInit(String),
    #[error("Configuration error: {0}")]
    Configuration(String),
    #[error("File Cache initialization failed: {0}")]
    FileCacheInit(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("{0} service is not configured")]
    NotConfigured(String),
}

pub struct ServiceRuntime {
    pub tokio_runtime: Arc<Runtime>,
    pub http_client: Option<Arc<dyn HttpClient>>,
    pub cookie_auto_save_handle: Option<Arc<Mutex<JoinHandle<()>>>>,
    pub storage_manager: Option<Arc<dyn StorageManager>>,
}

impl ServiceRuntime {
    pub fn with_tokio_runtime(
        config: RuntimeConfig,
        tokio_runtime: Arc<Runtime>,
    ) -> Result<Arc<Self>, InitError> {
        let cookie_store_initialization =
            Self::initialize_cookie_store(&tokio_runtime, config.cookie);
        let optional_cookie_store_initialization: Option<(
            Arc<dyn CookieStore>,
            Arc<Mutex<JoinHandle<()>>>,
        )>;
        if cookie_store_initialization.is_ok() {
            optional_cookie_store_initialization = Some(cookie_store_initialization?);
        } else {
            optional_cookie_store_initialization = None;
        }

        let mut cookie_store: Option<Arc<dyn CookieStore>> = None;
        let mut cookie_auto_save_handle: Option<Arc<Mutex<JoinHandle<()>>>> = None;

        if optional_cookie_store_initialization.is_some() {
            let cookie_store_initialize = optional_cookie_store_initialization.unwrap();
            cookie_store = Some(cookie_store_initialize.0);
            cookie_auto_save_handle = Some(cookie_store_initialize.1);
        }

        let http_client = if let Some(http_config) = config.http {
            let http_client = Self::create_http_client(http_config, cookie_store)?;
            Some(http_client)
        } else {
            None
        };

        let storage_manager = Self::create_storage_manager()?;

        Ok(Arc::new(Self {
            tokio_runtime,
            http_client,
            cookie_auto_save_handle,
            storage_manager: Some(storage_manager),
        }))
    }

    pub fn available_runtime(&self) -> Arc<Runtime> {
        self.tokio_runtime.clone()
    }

    pub fn execute_block<F, R>(&self, future: F) -> R
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        self.available_runtime().block_on(future)
    }

    pub fn execute_async_blocking<F, R>(&self, func: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.available_runtime().spawn_blocking(func)
    }

    pub fn execute_async<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.available_runtime().spawn(future)
    }
    
    pub fn execute_http(
        &self,
        endpoint: HttpEndpoint,
    ) -> Result<JoinHandle<Result<HttpResponse, HttpClientError>>, ServiceError> {
        if self.http_client.is_none() {
            return Err(ServiceError::NotConfigured("Http Client".to_string()));
        }
        let client = self.http_client.as_ref().unwrap().clone();
        Ok(self.execute_async(async move { client.execute(endpoint).await }))
    }

    pub fn execute_stream_http(
        &self,
        endpoint: HttpEndpoint,
    ) -> Result<JoinHandle<Result<HttpStreamResponse, HttpClientError>>, ServiceError> {
        if self.http_client.is_none() {
            return Err(ServiceError::NotConfigured("Http Client".to_string()));
        }

        let client = self.http_client.as_ref().unwrap().clone();
        Ok(self.execute_async(async move { client.execute_stream(endpoint).await }))
    }

    pub async fn read_file(
        &self,
        read_file: ReadFile,
    ) -> Result<Result<Vec<u8>, StorageError>, ServiceError> {
        if self.storage_manager.is_none() {
            return Err(ServiceError::NotConfigured("Storage Manager".to_string()));
        }

        let storage_manager = self.storage_manager.as_ref().unwrap();
        Ok(storage_manager.read(read_file).await)
    }

    pub async fn write_file<'a>(
        &self,
        write_file: WriteFile<'a>,
    ) -> Result<Result<(), StorageError>, ServiceError> {
        if self.storage_manager.is_none() {
            return Err(ServiceError::NotConfigured("Storage Manager".to_string()));
        }

        let storage_manager = self.storage_manager.as_ref().unwrap();
        Ok(storage_manager.write(write_file).await)
    }

    pub fn spawn_handle(&self) -> tokio::runtime::Handle {
        self.available_runtime().handle().clone()
    }

    fn initialize_cookie_store(
        tokio_runtime: &Runtime,
        config: Option<CookieConfig>,
    ) -> Result<(Arc<dyn CookieStore>, Arc<Mutex<JoinHandle<()>>>), InitError> {
        let cookie_store_option = if let Some(cookie_config) = config {
            Some(tokio_runtime.block_on(async {
                let cookie_store = Self::create_cookie_store(cookie_config).await?;
                Ok::<_, InitError>(cookie_store)
            }))
        } else {
            return Err(InitError::Configuration("config is null".to_string()));
        };

        let cookie_store = if let Some(cookie_store) = cookie_store_option {
            if cookie_store.is_err() {
                return Err(cookie_store.err().unwrap());
            } else {
                Some(cookie_store?)
            }
        } else {
            return Err(InitError::Configuration("cookie store is null".to_string()));
        };

        let cookie_auto_save_handle = if let Some(cookie_store) = &cookie_store {
            let unwrapped = cookie_store.clone();
            let file_backend_cookie_store = unwrapped.downcast_arc::<FileBackedCookieStore>();
            if let Some(file_backend_cookie_store) = file_backend_cookie_store {
                let handle =
                    tokio_runtime.block_on(async { file_backend_cookie_store.start_auto_save() });

                Some(Arc::new(Mutex::new(handle)))
            } else {
                return Err(InitError::Configuration(
                    "file cookie store is null".to_string(),
                ));
            }
        } else {
            return Err(InitError::Configuration("cookie store is null".to_string()));
        };

        Ok((cookie_store.unwrap(), cookie_auto_save_handle.unwrap()))
    }

    async fn create_cookie_store(
        cookie_config: CookieConfig,
    ) -> Result<Arc<dyn CookieStore>, InitError> {
        let store = FileBackedCookieStore::new(cookie_config)
            .await
            .map_err(|e| InitError::Configuration(e.to_string()))?;

        let store = Arc::new(store);
        Ok(store)
    }

    fn create_http_client(
        http_config: HttpConfig,
        cookie_store: Option<Arc<dyn CookieStore>>,
    ) -> Result<Arc<dyn HttpClient>, InitError> {
        let backend = ReqwestBackend::with_parameters(http_config, cookie_store)
            .map_err(|e| InitError::HttpClientInit(e.to_string()))?;

        Ok(Arc::new(backend))
    }

    fn create_storage_manager() -> Result<Arc<dyn StorageManager>, InitError> {
        let backend = AsyncStorageManager::new();
        Ok(Arc::new(backend))
    }
}
