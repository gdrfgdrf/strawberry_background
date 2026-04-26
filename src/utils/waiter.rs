use parking_lot::{Condvar, Mutex};
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutError;

impl std::fmt::Display for TimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wait timed out")
    }
}

impl std::error::Error for TimeoutError {}

pub struct OptionWaiter<T> {
    value: Mutex<Option<Arc<T>>>,
    condvar: Condvar,
}

impl<T> OptionWaiter<T> {
    pub fn new() -> Self {
        Self {
            value: Mutex::new(None),
            condvar: Condvar::new(),
        }
    }

    pub fn clear_and_wait_timeout(&self, timeout: Duration) -> Result<Arc<T>, TimeoutError> {
        {
            let mut guard = self.value.lock();
            *guard = None;
        }
        self.wait_timeout(timeout)
    }

    pub fn wait_timeout(&self, timeout: Duration) -> Result<Arc<T>, TimeoutError> {
        let mut guard = self.value.lock();
        let start = std::time::Instant::now();
        loop {
            if let Some(val) = guard.as_ref() {
                return Ok(val.clone());
            }
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return Err(TimeoutError);
            }
            let remaining = timeout - elapsed;
            let wait_result = self.condvar.wait_for(&mut guard, remaining);
            if wait_result.timed_out() {
                if let Some(val) = guard.as_ref() {
                    return Ok(val.clone());
                }
                return Err(TimeoutError);
            }
        }
    }

    pub fn clear(&self) {
        let mut guard = self.value.lock();
        *guard = None;
    }

    pub fn set(&self, val: Arc<T>) {
        let mut guard = self.value.lock();
        *guard = Some(val);
        self.condvar.notify_one();
    }
}

pub struct BoolWaiter {
    value: Mutex<bool>,
    condvar: Condvar,
}

impl BoolWaiter {
    pub fn new(value: bool) -> Self {
        Self {
            value: Mutex::new(value),
            condvar: Condvar::new(),
        }
    }

    pub fn reset_and_wait_timeout(
        &self,
        reset: bool,
        target: bool,
        timeout: Duration,
    ) -> Result<bool, TimeoutError> {
        {
            let mut guard = self.value.lock();
            *guard = reset;
        }
        self.wait_timeout(target, timeout)
    }

    pub fn wait_timeout(&self, target: bool, timeout: Duration) -> Result<bool, TimeoutError> {
        let mut guard = self.value.lock();
        let start = std::time::Instant::now();
        loop {
            if guard.deref() == &target {
                return Ok(target);
            }

            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return Err(TimeoutError);
            }
            let remaining = timeout - elapsed;
            let wait_result = self.condvar.wait_for(&mut guard, remaining);
            if wait_result.timed_out() {
                if guard.deref() == &target {
                    return Ok(target);
                }
                return Err(TimeoutError);
            }
        }
    }

    pub fn reset(&self, val: bool) {
        let mut guard = self.value.lock();
        if guard.deref() == &val {
            return;
        }

        *guard = val;
    }

    pub fn set(&self, val: bool) {
        self.reset(val);
        self.condvar.notify_one();
    }
}
