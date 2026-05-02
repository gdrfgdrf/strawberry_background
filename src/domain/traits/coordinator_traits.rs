use crate::domain::models::coordinator_models::{
    CategorizerError, CoordinatorError, CycleSnapshot, DiscoverError, Identifier,
    PostRunnerConfiguration, QueuerError, Request, RunnerConfiguration, RunnerError,
    RunnerSnapshot,
};
use bytes::Bytes;
use std::sync::Arc;
use std::time::Duration;

pub trait Coordinator {
    fn cycle_once(&self) -> Result<Arc<CycleSnapshot>, CoordinatorError>;
    fn put(&self, request: Request) -> Result<(), CoordinatorError>;
}

pub trait Queuer: Send + Sync + 'static {
    fn cycle_once(&self) -> Result<(), QueuerError>;
    fn put(&self, request: Request) -> Result<(), QueuerError>;
}

pub trait Runner: Send + Sync + 'static {
    fn identifier(&self) -> &Identifier;
    fn configuration(&self) -> &RunnerConfiguration;
    fn cycle_once(&self) -> Result<RunnerSnapshot, RunnerError>;
    fn submit(&self, request: Request, watcher: Arc<dyn RunnerWatcher>) -> Result<(), RunnerError>;
}

pub trait PostRunner: Send + Sync + 'static {
    fn identifier(&self) -> &Identifier;
    fn configuration(&self) -> &PostRunnerConfiguration;
    fn post(&self, bytes: Bytes);
    fn post_err(&self, err: RunnerError);
}

pub trait RunnerDiscover: Send + Sync + 'static {
    fn cycle_once(&self, snapshot: &Arc<CycleSnapshot>) -> Result<(), DiscoverError>;
    fn discover_runner_by_category(
        &self,
        category: &String,
        timeout: Duration,
    ) -> Result<Identifier, DiscoverError>;
}

pub trait Categorizer: Send + Sync + 'static {
    fn categorize(&self, request: &Request) -> Result<String, CategorizerError>;
}

pub trait RunnerWatcher: Send + Sync + 'static {
    fn on_result(&self, bytes: Option<Bytes>);
    fn on_error(&self, err: RunnerError);
    fn on_progress(&self, value: u64, total: Option<u64>);
}

pub trait ProgressListenerManager: Send + Sync + 'static {
    fn add_listener(&self, identifier: Identifier, listener: Arc<dyn ProgressListener>);
    fn remove_listener(&self, identifier: &Identifier);

    fn notify_success(&self, identifier: &Identifier);
    fn notify_fail(&self, identifier: &Identifier, err: &RunnerError);
    fn notify_progress(&self, request: &Identifier, value: u64, total: Option<u64>);
}

pub trait ProgressListener: Send + Sync {
    fn on_progress(&self, identifier: &Identifier, value: u64, total: Option<u64>);
    fn on_success(&self, identifier: &Identifier);
    fn on_fail(&self, identifier: &Identifier, err: &RunnerError);
}
