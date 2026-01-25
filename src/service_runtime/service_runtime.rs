use crate::domain::traits::{DecryptionProvider, EncryptionProvider, HttpClient};
use crate::infrastructure::http::reqwest_backend::ReqwestBackend;
use crate::service_runtime::config::{HttpConfig, RuntimeConfig, TokioConfig};
use std::sync::Arc;
use tokio::runtime::Runtime;
use crate::domain::models::{HttpClientError, HttpEndpoint, HttpResponse};

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
    pub tokio_runtime: Runtime,
    pub http_client: Arc<dyn HttpClient>,
}

impl ServiceRuntime {
    pub fn initialize(config: RuntimeConfig) -> Result<Arc<Self>, InitError> {
        let tokio_runtime = Self::create_tokio_runtime(config.tokio)?;

        let http_client = tokio_runtime.block_on(async {
            let http_client = Self::create_http_client(config.http).await?;
            Ok::<_, InitError>(http_client)
        })?;

        Ok(Arc::new(Self {
            tokio_runtime,
            http_client,
        }))
    }

    pub fn execute_async<F, R>(&self, future: F) -> R
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        self.tokio_runtime.block_on(future)
    }

    pub fn execute_http(
        &self,
        endpoint: HttpEndpoint,
    ) -> tokio::task::JoinHandle<Result<HttpResponse, HttpClientError>> {
        let client = Arc::clone(&self.http_client);

        self.tokio_runtime.spawn(async move {
            client.execute(endpoint).await
        })
    }

    pub fn spawn_handle(&self) -> tokio::runtime::Handle {
        self.tokio_runtime.handle().clone()
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

    async fn create_http_client(http_config: HttpConfig) -> Result<Arc<dyn HttpClient>, InitError> {
        let backend = ReqwestBackend::with_config(http_config)
            .map_err(|e| InitError::HttpClientInit(e.to_string()))?;

        Ok(Arc::new(backend))
    }
}
