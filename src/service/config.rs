use std::sync::Arc;
use std::time::Duration;
use crate::domain::traits::http_traits::{DecryptionProvider, EncryptionProvider};

pub struct RuntimeConfig {
    pub tokio: TokioConfig,
    pub http: Option<HttpConfig>,
    pub cookie_config: Option<CookieConfig>
}

#[derive(Debug, Clone)]
pub struct TokioConfig {
    pub worker_threads: Option<usize>,
    pub thread_stack_size: Option<usize>,
    pub thread_name_prefix: Option<String>,
}

pub struct HttpConfig {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub pool_idle_timeout: Duration,
    pub max_connections_per_host: usize,
    pub cookie_config: Option<CookieConfig>,
    pub encryption_provider: Option<Arc<dyn EncryptionProvider>>,
    pub decryption_provider: Option<Arc<dyn DecryptionProvider>>
}

#[derive(Debug, Clone)]
pub struct CookieConfig {
    pub cookie_path: Option<String>,
    pub debounce_delay: Duration,
    pub auto_save_interval: Option<Duration>
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            tokio: TokioConfig {
                worker_threads: None,
                thread_stack_size: None,
                thread_name_prefix: None,
            },
            http: None,
            cookie_config: None
        }
    }
}
