use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::time::Duration;
use bytes::Bytes;

#[derive(Debug, thiserror::Error)]
pub enum CoordinatorError {
    #[error("Error Forward: {0}")]
    ErrorForward(String),
    #[error("Runners Are Busy")]
    RunnersAreBusy,
    #[error("No Snapshot Available")]
    NoSnapshotAvailable,
    #[error("Wait For Runner Timeout")]
    WaitForRunnerTimeout,
    #[error("No Runners Available")]
    NoRunnersAvailable,
}

#[derive(Debug, thiserror::Error)]
pub enum QueuerError {
    #[error("Error Forward: {0}")]
    ErrorForward(String),
    #[error("Request is discarded")]
    RequestDiscarded,
    #[error("Wait Timeout")]
    WaitTimeout,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum RunnerError {
    #[error("Error Forward: {0}")]
    ErrorForward(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Cannot Find Runner By Identifier: {0}")]
    CannotFindRunnerByIdentifier(Identifier),
    #[error("Cannot Find Post Runner By Identifier: {0}")]
    CannotFindPostRunnerByIdentifier(Identifier),
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("Error Forward: {0}")]
    ErrorForward(String),
    #[error("Category Not Exists: {0}")]
    CategoryNotExists(String),
    #[error("No Available Runners Discovered")]
    NoAvailableRunnersDiscovered,
}

#[derive(Debug, thiserror::Error)]
pub enum CategorizerError {
    #[error("Error Forward: {0}")]
    ErrorForward(String),
}

#[derive(Clone)]
pub enum Priority {
    Top { order: Option<usize> },
    Normal { order: Option<usize> },
    Bottom { order: Option<usize> },
}

#[derive(Clone)]
pub enum RetryStrategy {
    RetryImmediately {
        max_retry: Option<usize>,
    },
    RetryFixed {
        max_retry: Option<usize>,
        delay: Duration,
    },
    RetryExponentialBackoff {
        max_retry: Option<usize>,
        initial: Duration,
        base: f32,
        max_delay: Duration,
    },
}

#[derive(Clone, PartialEq)]
pub enum RejectStrategy {
    Discard,
    Wait,
}

#[derive(Eq, PartialEq, Clone)]
pub enum RunnerStatus {
    Idle,
    Working,
    Busy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Identifier {
    pub id: String,
}

pub struct Progress {
    pub value: u64,
    pub total: u64,
}

#[derive(Clone)]
pub struct Request {
    pub identifier: Identifier,
    pub priority: Option<Priority>,
    pub retry_strategy: Option<RetryStrategy>,
    pub post_retry_strategy: Option<RetryStrategy>,
    pub timeout: Option<Duration>,
    pub bytes: Option<Bytes>
}

pub struct CycleSnapshot {
    pub runner_snapshots: Vec<Result<RunnerSnapshot, RunnerError>>,
}

pub struct RunnerSnapshot {
    pub identifier: Identifier,
    pub status: RunnerStatus,
}

pub struct PostRunnerSnapshot {
    pub identifier: Identifier,
    pub retry_count: Option<usize>,
    pub progress: Option<Progress>,
}

#[derive(Clone)]
pub struct GlobalConfiguration {
    pub max_retry: Option<usize>,
}

#[derive(Clone)]
pub struct CoordinatorConfiguration {
    pub cycle_interval: Option<Duration>,
    pub queue_configuration: Option<QueueConfiguration>,
}

#[derive(Clone)]
pub struct QueueConfiguration {
    pub cycle_interval: Option<Duration>,
    pub max_request_count: Option<usize>,
    pub reject_strategy: Option<RejectStrategy>,
    pub wait_for_runner_timeout: Option<Duration>,
    pub wait_for_queue_not_empty_timeout: Option<Duration>,
    pub wait_for_queue_not_full_timeout: Option<Duration>,
}

pub struct CategorizerConfiguration {}

pub struct RunnerConfiguration {
    pub identifier: Identifier,
    pub accepted_categories: Option<Vec<String>>,
}

pub struct PostRunnerConfiguration {
    pub identifier: Identifier,
    pub accepted_categories: Option<Vec<String>>,
}

impl Priority {
    fn variant_rank(&self) -> usize {
        match self {
            Priority::Top { .. } => 2,
            Priority::Normal { .. } => 1,
            Priority::Bottom { .. } => 0,
        }
    }

    fn order(&self) -> Option<usize> {
        match self {
            Priority::Top { order } => *order,
            Priority::Normal { order } => *order,
            Priority::Bottom { order } => *order,
        }
    }
}

impl Display for Identifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Identifier(id={})", self.id)
    }
}

impl PartialEq for Priority {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Priority {}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_variant_rank = self.variant_rank();
        let other_variant_rank = other.variant_rank();
        match self_variant_rank.cmp(&other_variant_rank) {
            Ordering::Equal => {
                let self_order = self.order();
                let other_order = other.order();
                let self_val = self_order.unwrap_or(usize::MAX);
                let other_val = other_order.unwrap_or(usize::MAX);
                other_val.cmp(&self_val)
            }
            other_ordering => other_ordering,
        }
    }
}

impl PartialEq for Request {
    fn eq(&self, other: &Self) -> bool {
        self.identifier == other.identifier
    }
}

impl Eq for Request {}

impl PartialOrd for Request {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Request {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.priority.is_none() && other.priority.is_none() {
            return Ordering::Equal
        }
        if self.priority.is_none() {
            return Ordering::Less
        }
        if other.priority.is_none() {
            return Ordering::Greater;
        }
        self.priority.cmp(&other.priority)
    }
}

impl Hash for Identifier {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl Into<String> for Identifier {
    fn into(self) -> String {
        self.id
    }
}

impl AsRef<String> for Identifier {
    fn as_ref(&self) -> &String {
        &self.id
    }
}

impl AsRef<str> for Identifier {
    fn as_ref(&self) -> &str {
        &self.id
    }
}

impl From<String> for Identifier {
    fn from(value: String) -> Self {
        Self { id: value }
    }
}

impl From<&str> for Identifier {
    fn from(value: &str) -> Self {
        Self { id: value.into() }
    }
}
