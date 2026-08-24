use crate::domain::models::coordinator_models::{
    Identifier, Request, RetryStrategy, RunnerConfiguration, RunnerError, RunnerSnapshot,
    RunnerStatus,
};
use crate::domain::traits::coordinator_traits::{Runner, RunnerWatcher};
use parking_lot::{Mutex, RwLock};
use rand::RngExt;
use rand::rngs::SmallRng;
use std::cmp::min;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::runtime::Runtime;

#[derive(Debug, Clone, thiserror::Error)]
pub enum BaseRunnerError {
    #[error("Concurrency Limitation")]
    ConcurrencyLimitation,
}

pub trait SimpleRunner {
    async fn submit<Watcher: RunnerWatcher>(
        &self,
        request: &Request,
        watcher: &Watcher,
    ) -> Result<(), RunnerError>;
}

pub struct BaseRunner<Runner: SimpleRunner> {
    tokio_runtime: Arc<Runtime>,
    identifier: Identifier,
    configuration: RunnerConfiguration,
    inner: Arc<Runner>,
    status_manager: Arc<StatusManager>,
}

struct StatusManager {
    max_concurrency_count: usize,
    status: RwLock<RunnerStatus>,
    ongoing_request_count: AtomicUsize,
}

struct RequestRetryer<Runner: SimpleRunner, Watcher: RunnerWatcher> {
    inner: Arc<Runner>,
    request: Request,
    watcher: Watcher,
    retry_count: Mutex<usize>,
}

impl<Runner: SimpleRunner> BaseRunner<Runner> {
    pub fn new(
        tokio_runtime: Arc<Runtime>,
        identifier: Identifier,
        configuration: RunnerConfiguration,
        inner: Runner,
        max_concurrency_count: usize,
    ) -> Self {
        let status_manager = Arc::new(StatusManager::new(max_concurrency_count));
        Self {
            tokio_runtime,
            identifier,
            configuration,
            inner: Arc::new(inner),
            status_manager,
        }
    }
}

impl<RunnerA: SimpleRunner> Runner for BaseRunner<RunnerA> {
    fn identifier(&self) -> &Identifier {
        &self.identifier
    }

    fn configuration(&self) -> &RunnerConfiguration {
        &self.configuration
    }

    fn cycle_once(&self) -> Result<RunnerSnapshot, RunnerError> {
        let status = self.status_manager.acquire_status();
        Ok(RunnerSnapshot {
            identifier: self.identifier.clone(),
            status,
        })
    }

    async fn submit<Watcher: RunnerWatcher>(
        &self,
        request: Request,
        watcher: Watcher,
    ) -> Result<(), RunnerError> {
        if !self.status_manager.allow_submission() {
            return Err(RunnerError::ErrorForward(
                BaseRunnerError::ConcurrencyLimitation.to_string(),
            ));
        }
        let inner = self.inner.clone();
        let status_manager = self.status_manager.clone();
        status_manager.increase_count();
        status_manager.update_status();

        let retryer = RequestRetryer::new(inner, request, watcher);
        retryer.start().await;

        status_manager.decrease_count();
        status_manager.update_status();

        Ok(())
    }
}

impl StatusManager {
    pub fn new(max_concurrency_count: usize) -> Self {
        Self {
            max_concurrency_count,
            status: RwLock::new(RunnerStatus::Idle),
            ongoing_request_count: AtomicUsize::new(0),
        }
    }

    pub fn allow_submission(&self) -> bool {
        let status = self.acquire_status();
        status == RunnerStatus::Idle || status == RunnerStatus::Working
    }

    pub fn increase_count(&self) {
        self.ongoing_request_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn decrease_count(&self) {
        self.ongoing_request_count.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn update_status(&self) {
        let count = self.ongoing_request_count.load(Ordering::SeqCst);
        if count <= 0 {
            self.change_status(RunnerStatus::Idle);
            return;
        }
        if count > 0 && count < self.max_concurrency_count {
            self.change_status(RunnerStatus::Working);
        }
        if count >= self.max_concurrency_count {
            self.change_status(RunnerStatus::Busy);
        }
    }

    pub fn acquire_status(&self) -> RunnerStatus {
        let guard = self.status.read();
        guard.deref().clone()
    }

    fn change_status(&self, target: RunnerStatus) {
        let guard = self.status.read();
        if *guard == target {
            return;
        }
        drop(guard);
        let mut guard = self.status.write();
        *guard = target;
    }
}

impl<Runner: SimpleRunner, Watcher: RunnerWatcher> RequestRetryer<Runner, Watcher> {
    pub fn new(inner: Arc<Runner>, request: Request, watcher: Watcher) -> Self {
        Self {
            inner,
            request,
            watcher,
            retry_count: Mutex::new(0),
        }
    }

    fn max_retry(&self) -> Option<usize> {
        let strategy = &self.request.retry_strategy;
        if strategy.is_none() {
            return None;
        }
        let strategy = strategy.as_ref().unwrap();
        match strategy {
            RetryStrategy::RetryImmediately { max_retry } => max_retry.clone(),
            RetryStrategy::RetryFixed { max_retry, .. } => max_retry.clone(),
            RetryStrategy::RetryExponentialBackoff { max_retry, .. } => max_retry.clone(),
        }
    }

    fn should_retry(&self) -> bool {
        let max_retry = self.max_retry();
        if max_retry.is_none() {
            return false;
        }

        let mut lock = self.retry_count.lock();
        *lock = *lock + 1;

        let max_retry = max_retry.unwrap();
        *lock <= max_retry
    }

    async fn sleep(&self) {
        let strategy = &self.request.retry_strategy;
        if strategy.is_none() {
            return;
        }
        let strategy = strategy.as_ref().unwrap();
        match strategy {
            RetryStrategy::RetryImmediately { .. } => {
                return;
            }
            RetryStrategy::RetryFixed { delay, .. } => {
                tokio::time::sleep(delay.clone()).await;
            }
            RetryStrategy::RetryExponentialBackoff {
                initial,
                base,
                max_delay,
                ..
            } => {
                let delay = {
                    let lock = self.retry_count.lock();
                    min(
                        initial.clone() * base.powi(*lock as i32) as u32,
                        max_delay.clone(),
                    )
                };
                let mut rng = rand::make_rng::<SmallRng>();
                let jitter = rng.random_range(-0.25..0.25);
                let delay = delay.mul_f64(jitter);
                tokio::time::sleep(delay).await;
            }
        }
    }

    pub async fn start(&self) {
        loop {
            let result = self.inner.submit(&self.request, &self.watcher).await;
            if result.is_ok() {
                return;
            }

            let should_retry = self.should_retry();
            if !should_retry {
                return;
            }
            self.sleep().await;
        }
    }
}
