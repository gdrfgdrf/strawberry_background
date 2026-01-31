use std::panic::AssertUnwindSafe;
use crate::domain::models::http_models::{HttpClientError, HttpEndpoint, HttpResponse};
use crate::domain::traits::cookie_traits::CookieStore;
use crate::domain::traits::http_traits::HttpClient;
use crate::infrastructure::http::reqwest_backend::ReqwestBackend;
use crate::service::config::{CookieConfig, HttpConfig, RuntimeConfig, TokioConfig};
use std::sync::Arc;
use tokio::runtime::Runtime;

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
}

impl ServiceRuntime {
    pub fn initialize(config: RuntimeConfig) -> Result<Arc<Self>, InitError> {
        let tokio_runtime = Self::create_tokio_runtime(config.tokio)?;

        let http_client = if let Some(http_config) = config.http {
            Some(tokio_runtime.block_on(async {
                let http_client =
                    Self::create_http_client(http_config, config.cookie_config).await?;
                Ok::<_, InitError>(http_client)
            })?)
        } else {
            None
        };

        Ok(Arc::new(Self {
            tokio_runtime: Some(tokio_runtime),
            provided_tokio_runtime: None,
            http_client,
        }))
    }
    
    pub fn with_tokio_runtime(config: RuntimeConfig, tokio_runtime: Arc<AssertUnwindSafe<Runtime>>) -> Result<Arc<Self>, InitError> {
        let http_client = if let Some(http_config) = config.http {
            Some(tokio_runtime.block_on(async {
                let http_client =
                    Self::create_http_client(http_config, config.cookie_config).await?;
                Ok::<_, InitError>(http_client)
            })?)
        } else {
            None
        };
        
        Ok(Arc::new(Self {
            tokio_runtime: None,
            provided_tokio_runtime: Some(tokio_runtime),
            http_client
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
    ) -> tokio::task::JoinHandle<Result<HttpResponse, HttpClientError>> {
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

    fn create_tokio_runtime(tokio_config: TokioConfig) -> Result<Runtime, InitError> {
        let mut builder = tokio::runtime::Builder::new_multi_thread();

        if let Some(threads) = tokio_config.worker_threads {
            builder.worker_threads(threads);
        }
        if let Some(stack_size) = tokio_config.thread_stack_size {
            builder.thread_stack_size(stack_size);
        }
        if let Some(prefix) = &tokio_config.thread_name_prefix {
            builder.thread_name(prefix);
        }

        builder
            .enable_all()
            .build()
            .map_err(|e| InitError::TokioInit(e.to_string()))
    }

    async fn create_http_client(
        http_config: HttpConfig,
        cookie_config: Option<CookieConfig>,
    ) -> Result<Arc<dyn HttpClient>, InitError> {
        let (cookie_store) = if let Some(cookie_config) = cookie_config {
            let runtime = tokio::runtime::Handle::current();
            let store = runtime
                .block_on(async {
                    crate::infrastructure::http::cookie_backend::FileBackedCookieStore::new(
                        cookie_config,
                    )
                    .await
                })
                .map_err(|_e| InitError::Configuration("cookie error".to_string()))?;

            let store = Arc::new(store);
            let _ = store.clone().start_auto_save();
            
            Some(store as Arc<dyn CookieStore>)
        } else {
            None
        };

        let backend = ReqwestBackend::with_parameters(http_config, cookie_store)
            .map_err(|e| InitError::HttpClientInit(e.to_string()))?;

        Ok(Arc::new(backend))
    }
}
