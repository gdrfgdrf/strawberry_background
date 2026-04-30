use crate::domain::traits::monitor_traits::Monitor;
use crate::service::config::RuntimeConfig;
use crate::service::service_runtime::{InitError, ServiceRuntime};
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

pub fn create_service_exporter_with_tokio_runtime(
    config: RuntimeConfig,
    tokio_runtime: Arc<Runtime>,
) -> Result<ServiceExporter, InitError> {
    let runtime = ServiceRuntime::with_tokio_runtime(config, tokio_runtime)?;
    Ok(ServiceExporter::new(runtime))
}

#[cfg(test)]
mod tests {
    use crate::domain::models::coordinator_models::{
        CategorizerError, CoordinatorConfiguration, Identifier, Priority, Request,
        RunnerConfiguration, RunnerError, RunnerSnapshot, RunnerStatus,
    };
    use crate::domain::models::http_models::{HttpEndpoint, HttpMethod};
    use crate::domain::models::storage_models::{EnsureMode, ReadFile, WriteFile, WriteMode};
    use crate::domain::traits::coordinator_traits::{
        Categorizer, Coordinator, Runner, RunnerWatcher,
    };
    use crate::rkv::rkv_impl::initialize_rkv;
    use crate::service::config::{
        CookieConfig, FileCacheChannelConfig, FileCacheConfig, HttpConfig, RuntimeConfig,
    };
    use crate::service::service_exporter::create_service_exporter_with_tokio_runtime;
    use crate::service::service_runtime::ServiceRuntime;
    use crate::superstructure::coordinator::coordinator::DefaultCoordinator;
    use crate::superstructure::coordinator::registry::RunnerRegistry;
    use parking_lot::Mutex;
    use std::ops::Deref;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime};
    use tokio::runtime::Runtime;
    use tokio_test::{assert_err, assert_ok};
    use tokio_util::sync::CancellationToken;

    macro_rules! await_test {
        ($e:expr) => {
            tokio_test::block_on($e)
        };
    }

    fn initialize_runtime() -> Arc<ServiceRuntime> {
        initialize_rkv("databases".into());
        let runtime = Runtime::new().unwrap();

        let service_exporter = create_service_exporter_with_tokio_runtime(
            RuntimeConfig {
                // tokio: TokioConfig {
                //     worker_threads: Some(4),
                //     thread_stack_size: None,
                //     thread_name_prefix: Some("strawberry-background-worker".to_string()),
                // },
                http: Some(HttpConfig {
                    connect_timeout: Duration::from_secs(10),
                    request_timeout: Duration::from_secs(30),
                    pool_idle_timeout: Duration::from_secs(90),
                    max_connections_per_host: 100,
                    encryption_provider: None,
                    decryption_provider: None,
                    cookie_config: None,
                    all_proxy: None,
                    host_proxy: None,
                    tls_danger_accept_invalid_certs: false,
                    tls_danger_accept_invalid_hostnames: false,
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
            },
            Arc::new(runtime),
        )
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
                    content_type: None,
                })
                .unwrap()
        )
        .unwrap()
        .unwrap();

        println!("response length: {}", response.body.len());

        // /// test cookie store
        // await_test!(async { loop {} });
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
                data: &data,
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

        let _ =
            await_test!(channel1.cache("test-tag".to_string(), "test-sentence".to_string(), &data))
                .unwrap();
        let fetched = await_test!(channel1.fetch(&"test-tag".to_string())).unwrap();

        assert_eq!(data, fetched);
    }

    #[test]
    fn test_file_cache_fetch() {
        let runtime = initialize_runtime();

        let data = "http world, this is the file cache test\n"
            .repeat(10086 ^ 2)
            .to_string()
            .into_bytes();

        let factory = runtime.file_cache_manager_factory.clone().unwrap();
        let channel1 = await_test!(factory.get_with_name(&"test-channel-1".to_string())).unwrap();

        for i in 0..10 {
            let fetched = await_test!(channel1.fetch(&format!("test-tag-{}", i))).unwrap();
            assert_eq!(data, fetched);
        }
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

        let _ =
            await_test!(channel2.cache("test-tag".to_string(), "test-sentence".to_string(), &data))
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

        let _ =
            await_test!(channel1.cache("test-tag".to_string(), "test-sentence".to_string(), &data))
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

        let runtime = initialize_runtime();
        for i in 0..10 {
            {
                let factory = runtime.file_cache_manager_factory.clone().unwrap();
                let channel1 =
                    await_test!(factory.get_with_name(&"test-channel-1".to_string())).unwrap();

                await_test!(channel1.cache(
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
            // {
            //     let runtime = initialize_runtime();
            //
            //     let factory = runtime.file_cache_manager_factory.clone().unwrap();
            //     let channel1 =
            //         await_test!(factory.get_with_name(&"test-channel-1".to_string())).unwrap();
            //
            //     let fetched = await_test!(channel1.fetch(&format!("test-tag-{}", i)));
            //     assert_ok!(&fetched);
            //
            //     let fetched = fetched.unwrap();
            //     assert_eq!(fetched, data);
            // }
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

    #[test]
    fn test_download_coordinator() {
        let service_runtime = initialize_runtime();
        let tokio_runtime = service_runtime.tokio_runtime.clone();

        {
            let runner_configuration_1 = RunnerConfiguration {
                identifier: Identifier {
                    id: "Runner-1".to_string(),
                },
                accepted_categories: None,
            };
            let runner_configuration_2 = RunnerConfiguration {
                identifier: Identifier {
                    id: "Runner-2".to_string(),
                },
                accepted_categories: Some(vec![
                    "second-request-requires-specific-runner".to_string(),
                ]),
            };
            let runner_1 = Arc::new(TestRunner {
                identifier: Identifier {
                    id: "Runner-1".to_string(),
                },
                configuration: runner_configuration_1,
                status: Mutex::new(RunnerStatus::Idle),
                test_cycle_count: Mutex::new(0),
                test_cycle_threshold: 10,
            });
            let runner_2 = Arc::new(TestRunner {
                identifier: Identifier {
                    id: "Runner-2".to_string(),
                },
                configuration: runner_configuration_2,
                status: Mutex::new(RunnerStatus::Idle),
                test_cycle_count: Mutex::new(0),
                test_cycle_threshold: 5,
            });

            let mut registry = RunnerRegistry::singleton().write();
            registry.put_runner(runner_1);
            registry.put_runner(runner_2);

            println!("runners are registered")
        }

        let coordinator_configuration = CoordinatorConfiguration {
            cycle_interval: None,
            queue_configuration: None,
        };
        let categorizer = Arc::new(TestCategorizer {});
        let coordinator = DefaultCoordinator::new(categorizer, coordinator_configuration);
        let coordinator_clone_1 = coordinator.clone();
        let coordinator_clone_2 = coordinator.clone();

        let cycler_cancellation_token_owned = Arc::new(CancellationToken::new());
        let cycler_cancellation_token_cloned = cycler_cancellation_token_owned.clone();
        let queuer_cancellation_token_owned = Arc::new(CancellationToken::new());
        let queuer_cancellation_token_cloned = queuer_cancellation_token_owned.clone();

        println!("starting cycler thread");
        std::thread::spawn(move || {
            println!("cycler thread started");
            coordinator_clone_1
                .cycler_thread_entrypoint(&cycler_cancellation_token_cloned, |err| {
                    println!("cycler err: {}", err)
                });
        });
        println!("starting queuer thread");
        std::thread::spawn(move || {
            println!("queuer thread started");
            coordinator_clone_2
                .queuer_thread_entrypoint(&queuer_cancellation_token_cloned, |err| {
                    println!("queuer err: {}", err)
                });
        });

        println!("sleep for 3 seconds");
        sleep(Duration::from_secs(3));

        println!("putting a request 1");
        let request = Request {
            identifier: Identifier {
                id: "first-request".to_string(),
            },
            priority: None,
            retry_strategy: None,
            post_retry_strategy: None,
            timeout: None,
        };
        coordinator.put(request).unwrap();

        println!("putting a request 2");
        let request = Request {
            identifier: Identifier {
                id: "second-request".to_string(),
            },
            priority: None,
            retry_strategy: None,
            post_retry_strategy: None,
            timeout: None,
        };
        coordinator.put(request).unwrap();

        println!("putting a request 3");
        let request = Request {
            identifier: Identifier {
                id: "third-request".to_string(),
            },
            priority: None,
            retry_strategy: None,
            post_retry_strategy: None,
            timeout: None,
        };
        coordinator.put(request).unwrap();

        println!("sleep for 3 seconds");
        sleep(Duration::from_secs(3));
        println!("cancelling cycler");
        cycler_cancellation_token_owned.cancel();
        println!("cancelling queuer");
        queuer_cancellation_token_owned.cancel();

        sleep(Duration::from_secs(30))
    }

    struct TestCategorizer {}
    struct TestRunner {
        identifier: Identifier,
        configuration: RunnerConfiguration,
        status: Mutex<RunnerStatus>,
        test_cycle_count: Mutex<usize>,
        test_cycle_threshold: usize,
    }

    impl Categorizer for TestCategorizer {
        fn categorize(&self, request: &Request) -> Result<String, CategorizerError> {
            let identifier = &request.identifier;
            if identifier.id == "second-request".to_string() {
                return Ok("second-request-requires-specific-runner".to_string());
            }
            Ok("omnipotence".to_string())
        }
    }

    impl Runner for TestRunner {
        fn identifier(&self) -> &Identifier {
            &self.identifier
        }

        fn configuration(&self) -> &RunnerConfiguration {
            &self.configuration
        }

        fn cycle_once(&self) -> Result<RunnerSnapshot, RunnerError> {
            println!("Runner {}: cycle once", self.identifier);

            let mut status = { self.status.lock().clone() };
            if status == RunnerStatus::Busy {
                let mut current = self.test_cycle_count.lock();
                *current = current.clone() + 1;

                if current.clone() >= self.test_cycle_threshold {
                    println!("Runner {}: change status to idle", self.identifier);
                    *self.status.lock() = RunnerStatus::Idle;
                    status = RunnerStatus::Idle;
                    *current = 0;
                }
            }

            Ok(RunnerSnapshot {
                identifier: self.identifier.clone(),
                retry_count: None,
                progress: None,
                status,
            })
        }

        fn submit(&self, request: Request, watcher: Arc<dyn RunnerWatcher>) {
            println!(
                "Runner {}: working on {}",
                self.identifier, request.identifier
            );
            *self.status.lock() = RunnerStatus::Busy
        }
    }
}
