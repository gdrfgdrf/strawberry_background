use crate::domain::models::coordinator_models::{
    Identifier, Request, RunnerConfiguration, RunnerError, RunnerSnapshot, RunnerStatus,
};
use crate::domain::traits::coordinator_traits::{Runner, RunnerWatcher};
use bytes::Bytes;
use parking_lot::RwLock;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::runtime::Runtime;

#[derive(Debug, Clone, thiserror::Error)]
pub enum BaseRunnerError {
    #[error("Concurrency not available")]
    ConcurrencyLimitation,
}

pub trait SimpleRunner: Send + Sync {
    fn submit(&self, request: Request, tracker: RunnerTracker);
}

pub struct BaseRunner {
    tokio_runtime: Arc<Runtime>,
    identifier: Identifier,
    configuration: RunnerConfiguration,
    inner: Arc<dyn SimpleRunner>,
    status_manager: Arc<StatusManager>,
}

pub struct RunnerTracker {
    watcher: Arc<dyn RunnerWatcher>,
    on_finished: Box<dyn FnOnce() + Send>,
}

struct StatusManager {
    max_concurrency_count: usize,
    status: RwLock<RunnerStatus>,
    ongoing_request_count: AtomicUsize,
}

impl BaseRunner {
    pub fn new(
        tokio_runtime: Arc<Runtime>,
        identifier: Identifier,
        configuration: RunnerConfiguration,
        inner: Arc<dyn SimpleRunner>,
        max_concurrency_count: usize,
    ) -> Self {
        let status_manager = Arc::new(StatusManager::new(max_concurrency_count));
        Self {
            tokio_runtime,
            identifier,
            configuration,
            inner,
            status_manager,
        }
    }
}

impl Runner for BaseRunner {
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

    fn submit(&self, request: Request, watcher: Arc<dyn RunnerWatcher>) -> Result<(), RunnerError> {
        if !self.status_manager.allow_submission() {
            return Err(RunnerError::ErrorForward(
                BaseRunnerError::ConcurrencyLimitation.to_string(),
            ));
        }
        let inner = self.inner.clone();
        let status_manager = self.status_manager.clone();
        status_manager.increase_count();
        status_manager.update_status();

        let tracker = RunnerTracker::new(
            watcher,
            Box::new(move || {
                status_manager.decrease_count();
                status_manager.update_status();
            }),
        );
        self.tokio_runtime.spawn(async move {
            inner.submit(request, tracker);
        });

        Ok(())
    }
}

impl RunnerTracker {
    fn new(watcher: Arc<dyn RunnerWatcher>, on_finished: Box<dyn FnOnce() + Send>) -> Self {
        Self {
            watcher,
            on_finished,
        }
    }

    pub fn on_result(self, bytes: Option<Bytes>) {
        self.watcher.on_result(bytes);
        (self.on_finished)();
    }

    pub fn on_error(self, err: RunnerError) {
        self.watcher.on_error(err);
        (self.on_finished)();
    }

    pub fn on_progress(&self, value: u64, total: Option<u64>) {
        self.watcher.on_progress(value, total);
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
