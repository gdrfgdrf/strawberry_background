use crate::service::config::RuntimeConfig;
use crate::service::service_runtime::{InitError, ServiceRuntime};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tokio::runtime::Runtime;

pub struct ServiceExporter {
    runtime: Arc<ServiceRuntime>,
}

impl ServiceExporter {
    pub fn new(runtime: Arc<ServiceRuntime>) -> Self {
        Self { runtime }
    }

    pub fn runtime(&self) -> &Arc<ServiceRuntime> {
        &self.runtime
    }
}

pub fn create_service_exporter(config: RuntimeConfig) -> Result<ServiceExporter, InitError> {
    let runtime = ServiceRuntime::initialize(config)?;
    Ok(ServiceExporter::new(runtime))
}

pub fn create_service_exporter_with_tokio_runtime(
    config: RuntimeConfig,
    tokio_runtime: Arc<AssertUnwindSafe<Runtime>>,
) -> Result<ServiceExporter, InitError> {
    let runtime = ServiceRuntime::with_tokio_runtime(config, tokio_runtime)?;
    Ok(ServiceExporter::new(runtime))
}

#[cfg(test)]
mod tests {
    use crate::domain::models::http_models::{HttpEndpoint, HttpMethod};
    use crate::domain::models::storage_models::{ReadFile, WriteFile};
    use crate::service::config::{CookieConfig, HttpConfig, RuntimeConfig, TokioConfig};
    use crate::service::service_exporter::create_service_exporter;
    use crate::service::service_runtime::ServiceRuntime;
    use std::sync::Arc;
    use std::time::Duration;

    macro_rules! await_test {
        ($e:expr) => {
            tokio_test::block_on($e)
        };
    }

    fn initialize_runtime() -> Arc<ServiceRuntime> {
        let service_exporter = create_service_exporter(RuntimeConfig {
            tokio: TokioConfig {
                worker_threads: Some(4),
                thread_stack_size: None,
                thread_name_prefix: Some("strawberry-background-worker".to_string()),
            },
            http: Some(HttpConfig {
                connect_timeout: Duration::from_secs(10),
                request_timeout: Duration::from_secs(30),
                pool_idle_timeout: Duration::from_secs(90),
                max_connections_per_host: 100,
                encryption_provider: None,
                decryption_provider: None,
                cookie_config: None,
                all_proxy: None,
            }),
            cookie: Some(CookieConfig {
                cookie_path: Some("test_cookie.json".to_string()),
                debounce_delay: Duration::from_secs(10),
                auto_save_interval: Some(Duration::from_secs(60)),
                initial_cookies: None,
            }),
        })
        .unwrap();
        let runtime = service_exporter.runtime;
        runtime
    }

    #[test]
    fn test_http() {
        let runtime = initialize_runtime();
        let response = await_test!(
            runtime
                .execute_http(HttpEndpoint {
                    path: "/search".to_string(),
                    domain: "https://cn.bing.com".to_string(),
                    body: None,
                    timeout: Duration::from_secs(60),
                    headers: None,
                    path_params: None,
                    query_params: Some(vec![("q".to_string(), "netease".to_string())]),
                    method: HttpMethod::Get,
                    requires_encryption: false,
                    requires_decryption: false,
                    user_agent: None,
                    content_type: None
                })
                .unwrap()
        )
        .unwrap()
        .unwrap();

        println!("response length: {}", response.body.len());

        /// test cookie store
        await_test!(async { loop {} });
    }

    #[test]
    fn test_storage() {
        let runtime = initialize_runtime();
        let path = "storage_test.txt".to_string();
        let data = "http world, this is the storage test"
            .to_string()
            .into_bytes();

        let _ = await_test!(
            runtime
                .write_file(WriteFile::path(path.clone(), data.clone()))
                .unwrap()
        )
        .unwrap()
        .unwrap();

        let read_data = await_test!(runtime.read_file(ReadFile::path(path)).unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(read_data, data)
    }
}
