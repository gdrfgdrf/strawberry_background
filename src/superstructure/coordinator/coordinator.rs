use crate::domain::models::coordinator_models::{
    CoordinatorConfiguration, CoordinatorError, CycleSnapshot, DiscoverError, Identifier,
    QueueConfiguration, QueuerError, RejectStrategy, Request, RunnerError, RunnerSnapshot,
    RunnerStatus,
};
use crate::domain::traits::coordinator_traits::{
    Categorizer, Coordinator, Queuer, Runner, RunnerDiscover, RunnerWatcher,
};
use crate::superstructure::coordinator::registry::RunnerRegistry;
use crate::utils::blocking_heap::BlockingHeap;
use crate::utils::waiter::{BoolWaiter, OptionWaiter};
use bytes::Bytes;
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub struct DefaultRunnerDiscover {
    snapshot_waiter: OptionWaiter<CycleSnapshot>,
    runner_notification_channels: DashMap<Identifier, Arc<BoolWaiter>>,
    category_notification_channels: DashMap<String, Arc<BoolWaiter>>,
    cached_runners_hash: RwLock<u64>,
}

pub struct DefaultCoordinator {
    configuration: CoordinatorConfiguration,
    snapshot: Mutex<Option<Arc<CycleSnapshot>>>,
    runner_discover: Arc<dyn RunnerDiscover>,
    queuer: Arc<dyn Queuer>,
}

pub struct DefaultQueuer {
    configuration: Option<QueueConfiguration>,
    runner_discover: Arc<dyn RunnerDiscover>,
    categorizer: Arc<dyn Categorizer>,
    queue: BlockingHeap<Request>,
}

pub struct DefaultRunnerWatcher {
    runner: Arc<dyn Runner>,
}

impl DefaultCoordinator {
    pub fn new(
        categorizer: Arc<dyn Categorizer>,
        configuration: CoordinatorConfiguration,
    ) -> Arc<Self> {
        let discover = Arc::new(DefaultRunnerDiscover::new());
        let queuer = Arc::new(DefaultQueuer::new(
            discover.clone(),
            categorizer,
            configuration.queue_configuration.clone(),
        ));

        let arc_coordinator = Arc::new(Self {
            configuration,
            snapshot: Mutex::new(None),
            runner_discover: discover,
            queuer,
        });
        arc_coordinator
    }

    pub fn cycler_thread_entrypoint<T>(
        &self,
        cancellation_token: &Arc<CancellationToken>,
        on_err: T,
    ) where
        T: Fn(CoordinatorError),
    {
        let interval = self
            .configuration
            .cycle_interval
            .unwrap_or(Duration::from_millis(100));

        loop {
            if cancellation_token.is_cancelled() {
                break;
            }

            let cycle_result = self.cycle_once();
            if cycle_result.is_err() {
                on_err(cycle_result.err().unwrap())
            }
            sleep(interval);
        }
    }

    pub fn queuer_thread_entrypoint<T>(
        &self,
        cancellation_token: &Arc<CancellationToken>,
        on_err: T,
    ) where
        T: Fn(QueuerError),
    {
        let interval = match &self.configuration.queue_configuration {
            None => Duration::from_millis(100),
            Some(configuration) => configuration
                .cycle_interval
                .unwrap_or(Duration::from_millis(100)),
        };

        loop {
            if cancellation_token.is_cancelled() {
                break;
            }

            let cycle_result = self.queuer.cycle_once();
            if cycle_result.is_err() {
                on_err(cycle_result.err().unwrap())
            }
            sleep(interval);
        }
    }
}

impl Coordinator for DefaultCoordinator {
    fn cycle_once(&self) -> Result<Arc<CycleSnapshot>, CoordinatorError> {
        let registry = RunnerRegistry::singleton().read();
        let runners = registry.all_runners();

        let mut runner_snapshots = Vec::<Result<RunnerSnapshot, RunnerError>>::new();
        runners.iter().for_each(|runner| {
            let cycle_once = runner.cycle_once();
            runner_snapshots.push(cycle_once);
        });

        let snapshot = Arc::new(CycleSnapshot { runner_snapshots });

        *self.snapshot.lock() = Some(snapshot.clone());
        let _ = self.runner_discover.cycle_once(&snapshot)?;
        Ok(snapshot)
    }

    fn put(&self, request: Request) -> Result<(), CoordinatorError> {
        self.queuer.put(request).map_err(|err| err.into())
    }
}

impl DefaultRunnerDiscover {
    fn new() -> Self {
        Self {
            snapshot_waiter: OptionWaiter::new(),
            runner_notification_channels: DashMap::new(),
            category_notification_channels: DashMap::new(),
            cached_runners_hash: RwLock::new(0u64),
        }
    }
}

impl RunnerDiscover for DefaultRunnerDiscover {
    fn cycle_once(&self, snapshot: &Arc<CycleSnapshot>) -> Result<(), DiscoverError> {
        let registry = RunnerRegistry::singleton().read();
        let latest_runners_hash = registry.runners_hash();

        if &latest_runners_hash != self.cached_runners_hash.read().deref() {
            let runners = registry.all_runners();
            runners.iter().for_each(|runner| {
                let configuration = runner.configuration();
                let identifier = &configuration.identifier;
                let categories = &configuration.accepted_categories;

                let mut waiter = self.runner_notification_channels.get(identifier);
                if waiter.is_none() {
                    let arc_waiter = Arc::new(BoolWaiter::new(false));
                    self.runner_notification_channels
                        .insert(identifier.clone(), arc_waiter);
                    waiter = self.runner_notification_channels.get(identifier);
                }
                let waiter = waiter.unwrap();

                if categories.is_none() {
                    let exists = self.category_notification_channels.get("omnipotence");
                    if exists.is_some() {
                        return;
                    }
                    self.category_notification_channels
                        .insert("omnipotence".to_string(), waiter.clone());
                    return;
                }
                let categories = categories.as_ref().unwrap();
                categories.iter().for_each(|category| {
                    let arc_waiter = self.category_notification_channels.get(category);
                    if arc_waiter.is_some() {
                        return;
                    }
                    self.category_notification_channels
                        .insert(category.clone(), waiter.clone());
                })
            });

            *self.cached_runners_hash.write() = latest_runners_hash;
        }

        let mut force_has_available = false;
        let mut all_is_available_map = HashMap::<String, bool>::new();
        snapshot
            .runner_snapshots
            .iter()
            .for_each(|runner_snapshot| {
                if force_has_available {
                    return;
                }
                if runner_snapshot.is_err() {
                    return;
                }
                let runner_snapshot = runner_snapshot.as_ref().ok().unwrap();
                let identifier = &runner_snapshot.identifier;
                let status = &runner_snapshot.status;
                let available = status == &RunnerStatus::Idle || status == &RunnerStatus::Working;

                let runner = registry.find_runner_by_identifier(identifier);
                if runner.is_err() {
                    return;
                }
                let runner = runner.unwrap();
                let configuration = runner.configuration();
                let categories = &configuration.accepted_categories;

                if categories.is_none() {
                    if available {
                        force_has_available = true;
                    }
                    return;
                }
                categories.as_ref().unwrap().iter().for_each(|category| {
                    let current = all_is_available_map.get(category);
                    if current.is_some() {
                        let current = current.unwrap();
                        if !current && available {
                            all_is_available_map.insert(category.clone(), available);
                        }
                    } else {
                        all_is_available_map.insert(category.clone(), available);
                    }
                });
            });

        if force_has_available {
            self.category_notification_channels
                .iter()
                .for_each(|reference| {
                    let waiter = reference.value();
                    waiter.set(true);
                });
        } else {
            all_is_available_map
                .iter()
                .for_each(|(category, available)| {
                    let waiter = self.category_notification_channels.get(category);
                    if waiter.is_none() {
                        return;
                    }
                    waiter.unwrap().set(available.clone())
                })
        }

        self.snapshot_waiter.set(snapshot.clone());
        Ok(())
    }

    fn discover_runner_by_category(
        &self,
        category: &String,
        timeout: Duration,
    ) -> Result<Identifier, DiscoverError> {
        let mut waiter = match category.is_empty() {
            true => {
                self.category_notification_channels.get("omnipotence")
            }
            false => {
                self.category_notification_channels.get(category)
            }
        };
        if waiter.is_none() {
            waiter = self.category_notification_channels.get("omnipotence");
            if waiter.is_none() {
                return Err(DiscoverError::CategoryNotExists(category.clone()));
            }
        }
        let waiter = waiter.unwrap();
        let _ = waiter.wait_timeout(true, timeout)?;
        waiter.reset(false);

        let snapshot = self.snapshot_waiter.wait_timeout(timeout.clone())?;
        self.snapshot_waiter.clear();

        let registry = RunnerRegistry::singleton().read();
        for snapshot_result in &snapshot.runner_snapshots {
            if snapshot_result.is_err() {
                continue;
            }
            let runner_snapshot = snapshot_result.as_ref().ok().unwrap();
            let identifier = &runner_snapshot.identifier;
            let status = &runner_snapshot.status;
            if status != &RunnerStatus::Idle && status != &RunnerStatus::Working {
                continue;
            }
            let runner = registry.find_runner_by_identifier(identifier)?;
            let configuration = runner.configuration();
            let categories = &configuration.accepted_categories;
            if categories.is_none() {
                return Ok(identifier.clone());
            }
            let categories = categories.as_ref().unwrap();
            if !categories.contains(category) {
                continue;
            }
            return Ok(identifier.clone());
        }

        Err(DiscoverError::NoAvailableRunnersDiscovered)
    }
}

impl DefaultQueuer {
    pub fn new(
        runner_discover: Arc<dyn RunnerDiscover>,
        categorizer: Arc<dyn Categorizer>,
        configuration: Option<QueueConfiguration>,
    ) -> Self {
        let max_request_count = match &configuration {
            None => 128,
            Some(configuration) => configuration.max_request_count.unwrap_or(128),
        };

        DefaultQueuer {
            configuration,
            runner_discover,
            categorizer,
            queue: BlockingHeap::with_capacity(max_request_count),
        }
    }
}

impl Queuer for DefaultQueuer {
    fn cycle_once(&self) -> Result<(), QueuerError> {
        let not_empty_timeout = match &self.configuration {
            None => Duration::from_secs(1),
            Some(configuration) => configuration
                .wait_for_queue_not_empty_timeout
                .unwrap_or(Duration::from_secs(1)),
        };

        let request = self.queue.pop(not_empty_timeout);
        if request.is_none() {
            return Ok(());
        }
        let request = request.unwrap();
        let category = self.categorizer.categorize(&request)?;
        let timeout = match &self.configuration {
            None => Duration::from_secs(1),
            Some(configuration) => configuration
                .wait_for_runner_timeout
                .unwrap_or(Duration::from_secs(1)),
        };
        let runner_identifier = self
            .runner_discover
            .discover_runner_by_category(&category, timeout)?;

        let registry = RunnerRegistry::singleton().read();
        let runner = registry.find_runner_by_identifier(&runner_identifier)?;
        let watcher = Arc::new(DefaultRunnerWatcher::new(runner.clone()));
        runner.submit(request, watcher);

        Ok(())
    }

    fn put(&self, request: Request) -> Result<(), QueuerError> {
        let default_not_full_timeout = Duration::from_secs(30);
        let (reject_strategy, not_full_timeout) = match &self.configuration {
            None => (&RejectStrategy::Wait, &default_not_full_timeout),
            Some(configuration) => (
                configuration
                    .reject_strategy
                    .as_ref()
                    .unwrap_or(&RejectStrategy::Wait),
                configuration
                    .wait_for_queue_not_full_timeout
                    .as_ref()
                    .unwrap_or(&default_not_full_timeout),
            ),
        };

        let capacity = self.queue.capacity();
        let length = self.queue.len();
        if length >= capacity && reject_strategy == &RejectStrategy::Discard {
            let result = self.queue.push(request, not_full_timeout.clone());
            return match result {
                Ok(_) => Ok(()),
                Err(_) => Err(QueuerError::WaitTimeout),
            };
        }
        let result = self.queue.push(request, not_full_timeout.clone());
        match result {
            Ok(_) => Ok(()),
            Err(_) => Err(QueuerError::WaitTimeout),
        }
    }
}

impl DefaultRunnerWatcher {
    pub fn new(runner: Arc<dyn Runner>) -> Self {
        Self { runner }
    }
}

impl RunnerWatcher for DefaultRunnerWatcher {
    fn on_result(&self, bytes: Bytes) {
        let empty_vec = Vec::<String>::new();
        let registry = RunnerRegistry::singleton().read();

        let configuration = self.runner.configuration();
        let accepted_categories = configuration
            .accepted_categories
            .as_ref()
            .unwrap_or(&empty_vec);
        if accepted_categories.is_empty() {
            registry.all_post_runners().iter().for_each(|post_runner| {
                post_runner.post(bytes.clone());
            });
            return;
        }

        let post_runners = registry.find_post_runners_by_categories(accepted_categories);
        if post_runners.is_none() {
            return;
        }
        let post_runners = post_runners.unwrap();
        post_runners.iter().for_each(|post_runner| {
            post_runner.post(bytes.clone());
        });
    }

    fn on_error(&self, err: RunnerError) {
        let empty_vec = Vec::<String>::new();
        let registry = RunnerRegistry::singleton().read();

        let configuration = self.runner.configuration();
        let accepted_categories = configuration
            .accepted_categories
            .as_ref()
            .unwrap_or(&empty_vec);
        if accepted_categories.is_empty() {
            registry.all_post_runners().iter().for_each(|post_runner| {
                post_runner.post_err(err.clone());
            });
            return;
        }

        let post_runners = registry.find_post_runners_by_categories(accepted_categories);
        if post_runners.is_none() {
            return;
        }
        let post_runners = post_runners.unwrap();
        post_runners
            .iter()
            .for_each(|post_runner| post_runner.post_err(err.clone()));
    }
}
