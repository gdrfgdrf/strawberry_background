use crate::domain::models::coordinator_models::{
    CoordinatorConfiguration, Identifier, Priority, QueueConfiguration, RejectStrategy, Request,
    RetryStrategy, RunnerConfiguration,
};
use crate::superstructure::coordinator::base::{BaseRunner, SimpleRunner};
use bytes::Bytes;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use strawberry_macros::builder;
use tokio::runtime::Runtime;

#[builder]
pub struct CoordinatorConfigurationBuilder {
    pub cycle_interval: Mutex<Option<Duration>>,
    pub queue_configuration_builder: Mutex<Option<QueueConfigurationBuilder>>,
}

#[builder]
pub struct QueueConfigurationBuilder {
    pub cycle_interval: Mutex<Option<Duration>>,
    pub max_request_count: Mutex<Option<usize>>,
    pub reject_strategy: Mutex<Option<RejectStrategy>>,
    pub wait_for_runner_timeout: Mutex<Option<Duration>>,
    pub wait_for_queue_not_empty_timeout: Mutex<Option<Duration>>,
    pub wait_for_queue_not_full_timeout: Mutex<Option<Duration>>,
}

#[builder]
pub struct RunnerConfigurationBuilder {
    pub identifier: Identifier,
    pub accepted_categories: Mutex<Option<Vec<String>>>,
}

#[builder]
pub struct RequestBuilder {
    pub identifier: Identifier,
    pub priority: Mutex<Option<Priority>>,
    pub retry_strategy: Mutex<Option<RetryStrategy>>,
    pub post_retry_strategy: Mutex<Option<RetryStrategy>>,
    pub timeout: Mutex<Option<Duration>>,
    pub bytes: Mutex<Option<Bytes>>,
}

#[builder]
pub struct BaseRunnerBuilder {
    pub tokio_runtime: Arc<Runtime>,
    pub identifier: Identifier,
    pub configuration_builder: RunnerConfigurationBuilder,
    pub inner: Arc<dyn SimpleRunner>,
    pub max_concurrency_count: Mutex<Option<usize>>,
}

impl CoordinatorConfigurationBuilder {
    pub fn build(self) -> CoordinatorConfiguration {
        CoordinatorConfiguration {
            cycle_interval: self.take_cycle_interval(),
            queue_configuration: self
                .take_queue_configuration_builder()
                .map(|configuration| configuration.into()),
        }
    }
}

impl QueueConfigurationBuilder {
    pub fn build(self) -> QueueConfiguration {
        QueueConfiguration {
            cycle_interval: self.take_cycle_interval(),
            max_request_count: self.take_max_request_count(),
            reject_strategy: self.take_reject_strategy(),
            wait_for_runner_timeout: self.take_wait_for_runner_timeout(),
            wait_for_queue_not_empty_timeout: self.take_wait_for_queue_not_empty_timeout(),
            wait_for_queue_not_full_timeout: self.take_wait_for_queue_not_full_timeout(),
        }
    }
}

impl RunnerConfigurationBuilder {
    pub fn build(self) -> RunnerConfiguration {
        RunnerConfiguration {
            accepted_categories: self.take_accepted_categories(),
            identifier: self.identifier,
        }
    }
}

impl RequestBuilder {
    pub fn build(self) -> Request {
        Request {
            priority: self.take_priority(),
            retry_strategy: self.take_retry_strategy(),
            post_retry_strategy: self.take_post_retry_strategy(),
            timeout: self.take_timeout(),
            bytes: self.take_bytes(),
            identifier: self.identifier,
        }
    }
}

impl BaseRunnerBuilder {
    pub fn build(self) -> BaseRunner {
        let max_concurrency_count = self.take_max_concurrency_count().unwrap_or(1);
        let tokio_runtime = self.tokio_runtime;
        let identifier = self.identifier;
        let configuration = self.configuration_builder.build();
        let inner = self.inner;
        BaseRunner::new(
            tokio_runtime,
            identifier,
            configuration,
            inner,
            max_concurrency_count,
        )
    }
}

impl Into<CoordinatorConfiguration> for CoordinatorConfigurationBuilder {
    fn into(self) -> CoordinatorConfiguration {
        self.build()
    }
}

impl Into<QueueConfiguration> for QueueConfigurationBuilder {
    fn into(self) -> QueueConfiguration {
        self.build()
    }
}

impl Into<RunnerConfiguration> for RunnerConfigurationBuilder {
    fn into(self) -> RunnerConfiguration {
        self.build()
    }
}

impl Into<Request> for RequestBuilder {
    fn into(self) -> Request {
        self.build()
    }
}
