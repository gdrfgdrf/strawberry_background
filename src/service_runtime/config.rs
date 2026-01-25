use std::time::Duration;
use crate::domain::traits::{DecryptionProvider, EncryptionProvider};

pub struct RuntimeConfig {
    pub tokio: TokioConfig,
    pub http: HttpConfig,
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
    pub encryption_provider: Option<Box<dyn EncryptionProvider>>,
    pub decryption_provider: Option<Box<dyn DecryptionProvider>>
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            tokio: TokioConfig {
                worker_threads: None,
                thread_stack_size: None,
                thread_name_prefix: None,
            },
            http: HttpConfig {
                connect_timeout: Duration::from_secs(10),
                request_timeout: Duration::from_secs(30),
                pool_idle_timeout: Duration::from_secs(90),
                max_connections_per_host: 100,
                encryption_provider: None,
                decryption_provider: None
            },
        }
    }
}
