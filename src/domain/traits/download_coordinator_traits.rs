use crate::domain::models::download_coordinator_models::{CategorizerError, CoordinatorError, CycleSnapshot, DiscoverError, Identifier, PostRunnerConfiguration, QueuerError, Request, RunnerConfiguration, RunnerError, RunnerSnapshot};
use std::sync::Arc;
use std::time::Duration;
use bytes::Bytes;

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
    fn submit(&self, request: Request, watcher: Arc<dyn RunnerWatcher>);
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
    fn on_result(&self, bytes: Bytes);
    fn on_error(&self, err: RunnerError);
}