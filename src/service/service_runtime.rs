use crate::domain::models::http_models::{HttpClientError, HttpEndpoint, HttpResponse};
use crate::domain::traits::cookie_traits::CookieStore;
use crate::domain::traits::http_traits::HttpClient;
use crate::infrastructure::http::cookie_backend::FileBackedCookieStore;
use crate::infrastructure::http::reqwest_backend::ReqwestBackend;
use crate::service::config::{CookieConfig, HttpConfig, RuntimeConfig, TokioConfig};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::runtime::{Handle, Runtime};
use tokio::task::JoinHandle;

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("Tokio runtime initialization failed: {0}")]
    TokioInit(String),
    #[error("HTTP client initialization failed: {0}")]
    HttpClientInit(String),
    #[error("Configuration error: {0}")]
    Configuration(String),
}

pub struct ServiceRuntime {
    pub tokio_runtime: Option<Runtime>,
    pub provided_tokio_runtime: Option<Arc<AssertUnwindSafe<Runtime>>>,
    pub http_client: Option<Arc<dyn HttpClient>>,
    pub cookie_auto_save_handle: Option<Arc<Mutex<JoinHandle<()>>>>,
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
            Some(tokio_runtime.block_on(async {
                let http_client = Self::create_http_client(http_config, cookie_store).await?;
                Ok::<_, InitError>(http_client)
            })?)
        } else {
            None
        };

        Ok(Arc::new(Self {
            tokio_runtime: Some(tokio_runtime),
            provided_tokio_runtime: None,
            http_client,
            cookie_auto_save_handle,
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
            Some(tokio_runtime.block_on(async {
                let http_client = Self::create_http_client(http_config, cookie_store).await?;
                Ok::<_, InitError>(http_client)
            })?)
        } else {
            None
        };

        Ok(Arc::new(Self {
            tokio_runtime: None,
            provided_tokio_runtime: Some(tokio_runtime),
            http_client,
            cookie_auto_save_handle,
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

    pub fn execute_async<F, R>(&self, future: F) -> R
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        self.available_runtime().block_on(future)
    }

    pub fn execute_http(
        &self,
        endpoint: HttpEndpoint,
    ) -> JoinHandle<Result<HttpResponse, HttpClientError>> {
        if self.http_client.is_none() {
            panic!("http is not configured");
        }

        let client = Arc::clone(&self.http_client.as_ref().unwrap());

        self.available_runtime()
            .spawn(async move { client.execute(endpoint).await })
    }

    pub fn spawn_handle(&self) -> tokio::runtime::Handle {
        self.available_runtime().handle().clone()
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
                let handle = tokio_runtime.block_on(async {
                    file_backend_cookie_store.start_auto_save()
                });

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

    async fn create_http_client(
        http_config: HttpConfig,
        cookie_store: Option<Arc<dyn CookieStore>>,
    ) -> Result<Arc<dyn HttpClient>, InitError> {
        let backend = ReqwestBackend::with_parameters(http_config, cookie_store)
            .map_err(|e| InitError::HttpClientInit(e.to_string()))?;

        Ok(Arc::new(backend))
    }
}
