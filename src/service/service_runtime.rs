use crate::domain::models::file_cache_models::CacheError;
use crate::domain::models::http_models::{HttpClientError, HttpEndpoint, HttpResponse};
use crate::domain::models::storage_models::{ReadFile, StorageError, WriteFile};
use crate::domain::traits::cookie_traits::CookieStore;
use crate::domain::traits::file_cache_traits::FileCacheManagerFactory;
use crate::domain::traits::http_traits::HttpClient;
use crate::domain::traits::storage_traits::StorageManager;
use crate::infrastructure::file_cache::file_cache_backend::{
    DefaultFileCacheManager, SingletonFileCacheManagerFactory,
};
use crate::infrastructure::http::cookie_backend::FileBackedCookieStore;
use crate::infrastructure::http::reqwest_backend::ReqwestBackend;
use crate::infrastructure::storage::storage_backend::AsyncStorageManager;
use crate::service::config::{
    CookieConfig, FileCacheConfig, HttpConfig, RuntimeConfig, TokioConfig,
};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    pub tokio_runtime: Option<Runtime>,
    pub provided_tokio_runtime: Option<Arc<AssertUnwindSafe<Runtime>>>,
    pub http_client: Option<Arc<dyn HttpClient>>,
    pub cookie_auto_save_handle: Option<Arc<Mutex<JoinHandle<()>>>>,
    pub storage_manager: Option<Arc<dyn StorageManager>>,
    pub file_cache_manager_factory: Option<Arc<dyn FileCacheManagerFactory>>,
}

impl ServiceRuntime {
    pub fn initialize(config: RuntimeConfig) -> Result<Arc<Self>, InitError> {
        let tokio_runtime = Self::create_tokio_runtime(config.tokio)?;

        let cookie_store_initialize_option =
            Self::initialize_cookie_store(&tokio_runtime, config.cookie);
        let mut cookie_store: Option<Arc<dyn CookieStore>> = None;
        let mut cookie_auto_save_handle: Option<Arc<Mutex<JoinHandle<()>>>> = None;

        if cookie_store_initialize_option.is_some() {
            let cookie_store_initialize = cookie_store_initialize_option.unwrap();
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
        let file_cache_manager_factory =
            Self::initialize_file_cache(&tokio_runtime, config.file_cache_config);

        Ok(Arc::new(Self {
            tokio_runtime: Some(tokio_runtime),
            provided_tokio_runtime: None,
            http_client,
            cookie_auto_save_handle,
            storage_manager: Some(storage_manager),
            file_cache_manager_factory,
        }))
    }

    pub fn with_tokio_runtime(
        config: RuntimeConfig,
        tokio_runtime: Arc<AssertUnwindSafe<Runtime>>,
    ) -> Result<Arc<Self>, InitError> {
        let cookie_store_initialize_option =
            Self::initialize_cookie_store(&tokio_runtime, config.cookie);
        let mut cookie_store: Option<Arc<dyn CookieStore>> = None;
        let mut cookie_auto_save_handle: Option<Arc<Mutex<JoinHandle<()>>>> = None;

        if cookie_store_initialize_option.is_some() {
            let cookie_store_initialize = cookie_store_initialize_option.unwrap();
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
        let file_cache_manager_factory =
            Self::initialize_file_cache(&tokio_runtime, config.file_cache_config);

        Ok(Arc::new(Self {
            tokio_runtime: None,
            provided_tokio_runtime: Some(tokio_runtime),
            http_client,
            cookie_auto_save_handle,
            storage_manager: Some(storage_manager),
            file_cache_manager_factory,
        }))
    }

    pub fn available_runtime(&self) -> &Runtime {
        if self.tokio_runtime.is_some() {
            return self.tokio_runtime.as_ref().unwrap();
        }
        if self.provided_tokio_runtime.is_some() {
            return self.provided_tokio_runtime.as_ref().unwrap();
        }
        panic!("no available runtime")
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

    pub async fn write_file(
        &self,
        write_file: WriteFile,
    ) -> Result<Result<(), StorageError>, ServiceError> {
        if self.storage_manager.is_none() {
            return Err(ServiceError::NotConfigured("Storage Manager".to_string()));
        }

        let storage_manager = self.storage_manager.as_ref().unwrap();
        Ok(storage_manager.write(write_file).await)
    }

    pub async fn file_cache_cache(
        &self,
        channel: &String,
        tag: String,
        sentence: String,
        bytes: &Vec<u8>,
    ) -> Result<Result<(), CacheError>, ServiceError> {
        if self.file_cache_manager_factory.is_none() {
            return Err(ServiceError::NotConfigured("File Cache".to_string()));
        }

        let file_cache_manager_factory = self.file_cache_manager_factory.as_ref().unwrap();
        let cache_manager = file_cache_manager_factory.get_with_name(channel).await;
        if cache_manager.is_err() {
            return Ok(cache_manager.map(|_| ()));
        }
        let cache_manager = cache_manager.unwrap();
        Ok(cache_manager.cache(tag, sentence, bytes).await)
    }

    pub async fn file_cache_should_update(
        &self,
        channel: &String,
        tag: &String,
        sentence: &String,
    ) -> Result<Result<bool, CacheError>, ServiceError> {
        if self.file_cache_manager_factory.is_none() {
            return Err(ServiceError::NotConfigured("File Cache".to_string()));
        }

        let file_cache_manager_factory = self.file_cache_manager_factory.as_ref().unwrap();
        let cache_manager = file_cache_manager_factory.get_with_name(channel).await;
        if cache_manager.is_err() {
            return Ok(cache_manager.map(|_| false));
        }
        let cache_manager = cache_manager.unwrap();
        Ok(cache_manager.should_update(tag, sentence).await)
    }

    pub async fn file_cache_fetch(
        &self,
        channel: &String,
        tag: &String,
    ) -> Result<Result<Vec<u8>, CacheError>, ServiceError> {
        if self.file_cache_manager_factory.is_none() {
            return Err(ServiceError::NotConfigured("File Cache".to_string()));
        }

        let file_cache_manager_factory = self.file_cache_manager_factory.as_ref().unwrap();
        let cache_manager = file_cache_manager_factory.get_with_name(channel).await;
        if cache_manager.is_err() {
            return Ok(cache_manager.map(|_| vec![]));
        }
        let cache_manager = cache_manager.unwrap();
        Ok(cache_manager.fetch(tag).await)
    }

    pub async fn file_cache_flush(
        &self,
        channel: &String,
        tag: &String,
    ) -> Result<Result<(), CacheError>, ServiceError> {
        if self.file_cache_manager_factory.is_none() {
            return Err(ServiceError::NotConfigured("File Cache".to_string()));
        }

        let file_cache_manager_factory = self.file_cache_manager_factory.as_ref().unwrap();
        let cache_manager = file_cache_manager_factory.get_with_name(channel).await;
        if cache_manager.is_err() {
            return Ok(cache_manager.map(|_| ()));
        }
        let cache_manager = cache_manager.unwrap();
        Ok(cache_manager.flush(tag).await)
    }

    pub async fn file_cache_persist(
        &self,
        channel: &String,
    ) -> Result<Result<(), CacheError>, ServiceError> {
        if self.file_cache_manager_factory.is_none() {
            return Err(ServiceError::NotConfigured("File Cache".to_string()));
        }

        let file_cache_manager_factory = self.file_cache_manager_factory.as_ref().unwrap();
        let cache_manager = file_cache_manager_factory.get_with_name(channel).await;
        if cache_manager.is_err() {
            return Ok(cache_manager.map(|_| ()));
        }
        let cache_manager = cache_manager.unwrap();
        Ok(cache_manager.persist().await)
    }

    pub fn spawn_handle(&self) -> tokio::runtime::Handle {
        self.available_runtime().handle().clone()
    }

    fn initialize_file_cache(
        tokio_runtime: &Runtime,
        config: Option<FileCacheConfig>,
    ) -> Option<Arc<dyn FileCacheManagerFactory>> {
        if config.is_none() {
            return None;
        }
        let config = config.unwrap();
        let factory =
            tokio_runtime.block_on(async { Self::create_file_cache_factory(config).await });
        if factory.is_err() {
            return None;
        }

        Some(factory.unwrap())
    }

    fn initialize_cookie_store(
        tokio_runtime: &Runtime,
        config: Option<CookieConfig>,
    ) -> Option<(Arc<dyn CookieStore>, Arc<Mutex<JoinHandle<()>>>)> {
        let cookie_store_option = if let Some(cookie_config) = config {
            Some(tokio_runtime.block_on(async {
                let cookie_store = Self::create_cookie_store(cookie_config).await?;
                Ok::<_, InitError>(cookie_store)
            }))
        } else {
            return None;
        };

        let cookie_store = if let Some(cookie_store) = cookie_store_option {
            if cookie_store.is_err() {
                return None;
            } else {
                Some(cookie_store.unwrap())
            }
        } else {
            return None;
        };
        let cookie_auto_save_handle = if let Some(cookie_store) = &cookie_store {
            let unwrapped = cookie_store.clone();
            let file_backend_cookie_store = unwrapped.downcast_arc::<FileBackedCookieStore>();
            if let Some(file_backend_cookie_store) = file_backend_cookie_store {
                let handle =
                    tokio_runtime.block_on(async { file_backend_cookie_store.start_auto_save() });

                Some(Arc::new(Mutex::new(handle)))
            } else {
                return None;
            }
        } else {
            return None;
        };

        Some((cookie_store?, cookie_auto_save_handle?))
    }

    fn create_tokio_runtime(tokio_config: TokioConfig) -> Result<Runtime, InitError> {
        let mut builder = tokio::runtime::Builder::new_multi_thread();

        if let Some(threads) = tokio_config.worker_threads {
            builder.worker_threads(threads);
        }
        if let Some(stack_size) = tokio_config.thread_stack_size {
            builder.thread_stack_size(stack_size);
        }
        if let Some(prefix) = tokio_config.thread_name_prefix {
            builder.thread_name_fn(move || {
                static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
                let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
                format!("{}-{}", prefix, id)
            });
        }

        builder
            .enable_all()
            .build()
            .map_err(|e| InitError::TokioInit(e.to_string()))
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

    async fn create_file_cache_factory(
        mut config: FileCacheConfig,
    ) -> Result<Arc<dyn FileCacheManagerFactory>, InitError> {
        let channels = config.channels.take();

        let factory = SingletonFileCacheManagerFactory::new(config, |config, channel| {
            let path = format!("{}/{}", config.base_path, channel.name);
            let manager = DefaultFileCacheManager::new(path, config.auto_save_interval, channel);
            let manager = Arc::new(manager);

            let _ = manager.clone().start_auto_save();
            manager
        });
        let factory = Arc::new(factory);

        if channels.is_some() {
            let channels = channels.unwrap();
            for channel_config in channels {
                let name = channel_config.name;
                let extension = channel_config.extension;

                let _ = factory
                    .create_with_name(name, extension)
                    .await
                    .map_err(|e| InitError::FileCacheInit(e.to_string()))?;
            }
        }

        Ok(factory)
    }
}
