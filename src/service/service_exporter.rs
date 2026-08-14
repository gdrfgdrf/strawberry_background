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
        CategorizerError, CoordinatorConfiguration, Identifier, Request,
        RunnerConfiguration, RunnerError, RunnerSnapshot, RunnerStatus,
    };
    use crate::domain::models::http_models::{HttpEndpoint, HttpMethod};
    use crate::domain::models::storage_models::{EnsureMode, ReadFile, WriteFile, WriteMode};
    use crate::domain::traits::coordinator_traits::{
        Categorizer, Coordinator, Runner, RunnerWatcher,
    };
    use crate::rkv::rkv_impl::initialize_rkv;
    use crate::service::config::{
        CookieConfig, HttpConfig, RuntimeConfig,
    };
    use crate::service::service_exporter::create_service_exporter_with_tokio_runtime;
    use crate::service::service_runtime::ServiceRuntime;
    use crate::superstructure::coordinator::coordinator::DefaultCoordinator;
    use crate::superstructure::coordinator::registry::RunnerRegistry;
    use parking_lot::Mutex;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime};
    use tokio::runtime::Runtime;
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
                    initial_cookies: None,
                }),
            },
            Arc::new(runtime),
        )
        .unwrap();
        let runtime = service_exporter.runtime;
        runtime
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
            bytes: None
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
            bytes: None
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
            bytes: None
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
                status,
            })
        }

        fn submit(&self, request: Request, watcher: Arc<dyn RunnerWatcher>) -> Result<(), RunnerError> {
            println!(
                "Runner {}: working on {}",
                self.identifier, request.identifier
            );
            *self.status.lock() = RunnerStatus::Busy;

            Ok(())
        }
    }
}
