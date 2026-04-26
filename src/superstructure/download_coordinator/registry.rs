use crate::domain::models::download_coordinator_models::{
    CycleSnapshot, Identifier, RegistryError,
};
use crate::domain::traits::download_coordinator_traits::{PostRunner, Runner};
use lazy_static::lazy_static;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

lazy_static! {
    static ref RUNNER_REGISTRY: RwLock<RunnerRegistry> = RwLock::new(RunnerRegistry::new());
}

pub struct RunnerRegistry {
    /// category -> Runners
    runners: HashMap<String, Vec<Arc<dyn Runner>>>,
    omnipotence_runners: Vec<Arc<dyn Runner>>,
    /// category -> Post Runners
    post_runners: HashMap<String, Vec<Arc<dyn PostRunner>>>,
    omnipotence_post_runners: Vec<Arc<dyn PostRunner>>,
    cached_runners_hash: u64,
    cached_post_runners_hash: u64,
}

impl RunnerRegistry {
    pub fn singleton() -> &'static RwLock<RunnerRegistry> {
        &RUNNER_REGISTRY
    }

    fn compute_runners_hash(
        data: &HashMap<String, Vec<Arc<dyn Runner>>>,
        omnipotence: &Vec<Arc<dyn Runner>>,
    ) -> u64 {
        let mut combined = 0u64;
        omnipotence.iter().for_each(|runner| {
            let mut hasher = DefaultHasher::new();
            let identifier = runner.identifier();
            identifier.hash(&mut hasher);
            let hash = hasher.finish();
            combined ^= hash;
        });
        data.values().for_each(|runners| {
            runners.iter().for_each(|runner| {
                let mut hasher = DefaultHasher::new();
                let identifier = runner.identifier();
                identifier.hash(&mut hasher);
                let hash = hasher.finish();
                combined ^= hash;
            })
        });

        combined
    }

    fn compute_post_runners_hash(
        data: &HashMap<String, Vec<Arc<dyn PostRunner>>>,
        omnipotence: &Vec<Arc<dyn PostRunner>>,
    ) -> u64 {
        let mut combined = 0u64;
        omnipotence.iter().for_each(|post_runner| {
            let mut hasher = DefaultHasher::new();
            let identifier = post_runner.identifier();
            identifier.hash(&mut hasher);
            let hash = hasher.finish();
            combined ^= hash;
        });
        data.values().for_each(|post_runners| {
            post_runners.iter().for_each(|runners| {
                let mut hasher = DefaultHasher::new();
                let configuration = runners.configuration();
                let identifier = &configuration.identifier;
                identifier.hash(&mut hasher);
                let hash = hasher.finish();
                combined ^= hash;
            })
        });

        combined
    }

    pub fn new() -> Self {
        Self {
            runners: HashMap::new(),
            omnipotence_runners: Vec::new(),
            post_runners: HashMap::new(),
            omnipotence_post_runners: Vec::new(),
            cached_runners_hash: 0u64,
            cached_post_runners_hash: 0u64,
        }
    }

    pub fn update_runner_hash(&mut self) {
        self.cached_runners_hash =
            Self::compute_runners_hash(&self.runners, &self.omnipotence_runners);
    }

    pub fn update_post_runner_hash(&mut self) {
        self.cached_post_runners_hash =
            Self::compute_post_runners_hash(&self.post_runners, &self.omnipotence_post_runners);
    }

    pub fn put_runner(&mut self, runner: Arc<dyn Runner>) {
        let empty_vec = Vec::new();
        let accepted_categories = runner
            .configuration()
            .accepted_categories
            .as_ref()
            .unwrap_or(&empty_vec);
        if accepted_categories.is_empty() {
            self.omnipotence_runners.push(runner);
            self.update_runner_hash();
            return;
        }
        accepted_categories.iter().for_each(|category| {
            let vec = self.runners.get_mut(category);
            if vec.is_none() {
                let mut vec = Vec::<Arc<dyn Runner>>::new();
                vec.push(runner.clone());
                self.runners.insert(category.clone(), vec);
                return;
            }
            let vec = vec.unwrap();
            vec.push(runner.clone());
        });

        self.update_runner_hash();
    }

    pub fn put_post_runners(&mut self, post_runner: Arc<dyn PostRunner>) {
        let empty_vec = Vec::new();
        let accepted_categories = post_runner
            .configuration()
            .accepted_categories
            .as_ref()
            .unwrap_or(&empty_vec);
        if accepted_categories.is_empty() {
            self.omnipotence_post_runners.push(post_runner);
            self.update_post_runner_hash();
            return;
        }
        accepted_categories.iter().for_each(|category| {
            let vec = self.post_runners.get_mut(category);
            if vec.is_none() {
                let mut vec = Vec::<Arc<dyn PostRunner>>::new();
                vec.push(post_runner.clone());
                self.post_runners.insert(category.clone(), vec);
                return;
            }
            let vec = vec.unwrap();
            vec.push(post_runner.clone());
        });

        self.update_post_runner_hash();
    }

    pub fn all_runners(&self) -> Vec<&Arc<dyn Runner>> {
        let mut result = Vec::<&Arc<dyn Runner>>::new();
        let mut identifiers = HashSet::<&Identifier>::new();
        self.omnipotence_runners.iter().for_each(|runner| {
            identifiers.insert(runner.identifier());
        });
        self.runners.iter().for_each(|(_, runners)| {
            runners.iter().for_each(|runner| {
                identifiers.insert(runner.identifier());
            })
        });

        identifiers.iter().for_each(|identifier| {
            let runner = self.find_runner_by_identifier(identifier);
            if runner.is_err() {
                return;
            }
            let runner = runner.unwrap();
            result.push(runner);
        });

        result
    }

    pub fn all_post_runners(&self) -> Vec<&Arc<dyn PostRunner>> {
        let mut result = Vec::<&Arc<dyn PostRunner>>::new();
        let mut identifiers = HashSet::<&Identifier>::new();
        self.omnipotence_post_runners
            .iter()
            .for_each(|post_runner| {
                identifiers.insert(post_runner.identifier());
            });
        self.post_runners.iter().for_each(|(_, post_runners)| {
            post_runners.iter().for_each(|post_runner| {
                identifiers.insert(post_runner.identifier());
            })
        });

        identifiers.iter().for_each(|identifier| {
            let post_runner = self.find_post_runner_by_identifier(identifier);
            if post_runner.is_err() {
                return;
            }
            let post_runner = post_runner.unwrap();
            result.push(post_runner);
        });

        result
    }

    pub fn all_runner_categories(&self) -> Vec<&String> {
        self.runners.keys().map(|key| key).collect()
    }

    pub fn all_post_runner_categories(&self) -> Vec<&String> {
        self.post_runners.keys().map(|key| key).collect()
    }

    pub fn find_runner_by_identifier(
        &self,
        identifier: &Identifier,
    ) -> Result<&Arc<dyn Runner>, RegistryError> {
        for runner in &self.omnipotence_runners {
            let runner_identifier = runner.identifier();
            if runner_identifier == identifier {
                return Ok(runner);
            }
        }

        let runners = self.runners.values();
        for runners in runners {
            for runner in runners {
                let runner_identifier = runner.identifier();
                if runner_identifier == identifier {
                    return Ok(runner);
                }
            }
        }
        Err(RegistryError::CannotFindRunnerByIdentifier(
            identifier.clone(),
        ))
    }

    pub fn find_post_runner_by_identifier(
        &self,
        identifier: &Identifier,
    ) -> Result<&Arc<dyn PostRunner>, RegistryError> {
        for post_runner in &self.omnipotence_post_runners {
            let post_runner_identifier = post_runner.identifier();
            if post_runner_identifier == identifier {
                return Ok(post_runner);
            }
        }

        let post_runners = self.post_runners.values();
        for post_runners in post_runners {
            for post_runner in post_runners {
                let post_runner_identifier = post_runner.identifier();
                if post_runner_identifier == identifier {
                    return Ok(post_runner);
                }
            }
        }

        Err(RegistryError::CannotFindPostRunnerByIdentifier(
            identifier.clone(),
        ))
    }

    pub fn find_post_runners_by_category(
        &self,
        category: &String,
    ) -> Option<Vec<&Arc<dyn PostRunner>>> {
        let mut result = Vec::<&Arc<dyn PostRunner>>::new();
        let mut identifiers = HashSet::<&Identifier>::new();
        let all_post_runners = self.all_post_runners();

        all_post_runners.iter().for_each(|post_runner| {
            let empty_vec = Vec::<String>::new();
            let identifier = post_runner.identifier();
            let configuration = post_runner.configuration();
            let accepted_categories = configuration
                .accepted_categories
                .as_ref()
                .unwrap_or(&empty_vec);
            if accepted_categories.is_empty() {
                identifiers.insert(identifier);
                return;
            }
            if accepted_categories.contains(category) {
                identifiers.insert(identifier);
            }
        });
        all_post_runners.iter().for_each(|post_runner| {
            let identifier = post_runner.identifier();
            if !identifiers.contains(identifier) {
                return;
            }
            result.push(post_runner);
        });

        if result.is_empty() {
            return None;
        }
        Some(result)
    }

    pub fn find_post_runners_by_categories(
        &self,
        categories: &Vec<String>,
    ) -> Option<Vec<&Arc<dyn PostRunner>>> {
        let mut result = Vec::<&Arc<dyn PostRunner>>::new();
        let mut identifiers = HashSet::<&Identifier>::new();
        let all_post_runners = self.all_post_runners();

        all_post_runners.iter().for_each(|post_runner| {
            let empty_vec = Vec::<String>::new();
            let identifier = post_runner.identifier();
            let configuration = post_runner.configuration();
            let accepted_categories = configuration
                .accepted_categories
                .as_ref()
                .unwrap_or(&empty_vec);
            if accepted_categories.is_empty() {
                identifiers.insert(identifier);
                return;
            }
            for category in categories {
                if !accepted_categories.contains(category) {
                    continue;
                }
                identifiers.insert(identifier);
                break;
            }
        });
        all_post_runners.iter().for_each(|post_runner| {
            let identifier = post_runner.identifier();
            if !identifiers.contains(identifier) {
                return;
            }
            result.push(post_runner);
        });

        if result.is_empty() {
            return None;
        }
        Some(result)
    }

    pub fn runners_hash(&self) -> u64 {
        self.cached_runners_hash
    }

    pub fn post_runners_hash(&self) -> u64 {
        self.cached_post_runners_hash
    }
}
