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
    use crate::domain::models::storage_models::{EnsureMode, ReadFile, WriteFile, WriteMode};
    use crate::service::config::{
        CookieConfig, FileCacheChannelConfig, FileCacheConfig, HttpConfig, RuntimeConfig,
        TokioConfig,
    };
    use crate::service::service_exporter::create_service_exporter;
    use crate::service::service_runtime::ServiceRuntime;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime};
    use tokio_test::{assert_err, assert_ok};

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
            file_cache_config: Some(FileCacheConfig {
                base_path: "file_cache_test".to_string(),
                auto_save_interval: Duration::from_secs(10),
                channels: Some(vec![
                    FileCacheChannelConfig {
                        name: "test-channel-1".to_string(),
                        extension: None,
                    },
                    FileCacheChannelConfig {
                        name: "test-channel-2".to_string(),
                        extension: Some("extension".to_string()),
                    },
                ]),
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

        let mut write_costs: Vec<f32> = Vec::new();
        let mut read_costs: Vec<f32> = Vec::new();
        for _ in 0..1000 {
            let path = "storage_test.txt".to_string();
            let data = "http world, this is the storage test\n"
                .repeat(10086 ^ 2)
                .to_string()
                .into_bytes();

            let current_time = SystemTime::now();
            let _ = await_test!(runtime.write_file(WriteFile {
                path: path.clone(),
                data: data.clone(),
                mode: WriteMode::Cover,
                timeout: Duration::from_secs(60),
                ensure_mode: Some(EnsureMode::SyncAll)
            }))
            .unwrap()
            .unwrap();

            write_costs.push(current_time.elapsed().unwrap().as_millis() as f32);

            let current_time = SystemTime::now();
            let read_data = await_test!(runtime.read_file(ReadFile::path(path)))
                .unwrap()
                .unwrap();

            read_costs.push(current_time.elapsed().unwrap().as_millis() as f32);

            assert_eq!(read_data.len(), data.len())
        }

        let write_sum: f32 = write_costs.iter().sum();
        let write_average = write_sum / write_costs.len() as f32;
        println!("write average: {:?}ms", write_average);

        let read_sum: f32 = read_costs.iter().sum();
        let read_average = read_sum / read_costs.len() as f32;
        println!("read average: {:?}ms", read_average);
    }

    #[test]
    fn test_file_cache_cache_fetch() {
        let runtime = initialize_runtime();

        let data = "http world, this is the file cache test\n"
            .repeat(10086 ^ 2)
            .to_string()
            .into_bytes();

        let factory = runtime.file_cache_manager_factory.clone().unwrap();
        let channel1 = await_test!(factory.get_with_name(&"test-channel-1".to_string())).unwrap();

        let _ = await_test!(channel1.cache(
            "test-tag".to_string(),
            "test-sentence".to_string(),
            &data
        ))
        .unwrap();
        let fetched = await_test!(channel1.fetch(&"test-tag".to_string())).unwrap();

        assert_eq!(data, fetched);
    }

    #[test]
    fn test_file_cache_cache_fetch_with_extension() {
        let runtime = initialize_runtime();

        let data = "http world, this is the file cache test\n"
            .repeat(10086 ^ 2)
            .to_string()
            .into_bytes();

        let factory = runtime.file_cache_manager_factory.clone().unwrap();
        let channel2 = await_test!(factory.get_with_name(&"test-channel-2".to_string())).unwrap();

        let _ = await_test!(channel2.cache(
            "test-tag".to_string(),
            "test-sentence".to_string(),
            &data
        ))
        .unwrap();
        let fetched = await_test!(channel2.fetch(&"test-tag".to_string())).unwrap();

        assert_eq!(data, fetched);
    }

    #[test]
    fn test_file_cache_cache_flush() {
        let runtime = initialize_runtime();

        let data = "http world, this is the file cache test\n"
            .repeat(10086 ^ 2)
            .to_string()
            .into_bytes();

        let factory = runtime.file_cache_manager_factory.clone().unwrap();
        let channel1 = await_test!(factory.get_with_name(&"test-channel-1".to_string())).unwrap();

        let _ = await_test!(channel1.cache(
            "test-tag".to_string(),
            "test-sentence".to_string(),
            &data
        ))
        .unwrap();

        let fetched = await_test!(channel1.fetch(&"test-tag".to_string())).unwrap();
        assert_eq!(data, fetched);

        let _ = await_test!(channel1.flush(&"test-tag".to_string())).unwrap();

        let fetched = await_test!(channel1.fetch(&"test-tag".to_string()));
        assert_err!(fetched);
    }

    #[test]
    fn test_file_cache_persist() {
        let data = "http world, this is the file cache test\n"
            .repeat(10086 ^ 2)
            .to_string()
            .into_bytes();

        for i in 0..10 {
            {
                let runtime = initialize_runtime();

                let factory = runtime.file_cache_manager_factory.clone().unwrap();
                let channel1 =
                    await_test!(factory.get_with_name(&"test-channel-1".to_string())).unwrap();

                let _ = await_test!(channel1.cache(
                    format!("test-tag-{}", i),
                    format!("test-sentence-{}", i),
                    &data
                ))
                .unwrap();

                let fetched = await_test!(channel1.fetch(&format!("test-tag-{}", i))).unwrap();
                assert_eq!(data, fetched);

                let persist = await_test!(channel1.persist());
                assert_ok!(persist);
            }
            {
                let runtime = initialize_runtime();

                let factory = runtime.file_cache_manager_factory.clone().unwrap();
                let channel1 =
                    await_test!(factory.get_with_name(&"test-channel-1".to_string())).unwrap();

                let fetched = await_test!(channel1.fetch(&format!("test-tag-{}", i)));
                assert_ok!(&fetched);

                let fetched = fetched.unwrap();
                assert_eq!(fetched, data);
            }
        }
    }

    #[test]
    fn test_file_cache_auto_save() {
        let data = "http world, this is the file cache test\n"
            .repeat(10086 ^ 2)
            .to_string()
            .into_bytes();

        let runtime = initialize_runtime();

        for _ in 0..2 {
            let factory = runtime.file_cache_manager_factory.clone().unwrap();
            let channel1 =
                await_test!(factory.get_with_name(&"test-channel-1".to_string())).unwrap();

            let _ = await_test!(channel1.cache(
                format!("test-tag-auto-save-{}", 0),
                format!("test-sentence-auto-save-{}", 0),
                &data
            ))
            .unwrap();

            let fetched =
                await_test!(channel1.fetch(&format!("test-tag-auto-save-{}", 0))).unwrap();
            assert_eq!(data, fetched);

            sleep(Duration::from_secs(10));
        }

        await_test!(async { loop {} });
    }
}
