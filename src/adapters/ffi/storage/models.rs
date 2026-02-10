use crate::domain::models::storage_models::{EnsureMode, ReadFile, WriteFile, WriteMode};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct FfiReadFile {
    pub path: String,
    pub timeout_millis: u64,
}

#[derive(Clone)]
pub struct FfiWriteFile {
    pub path: String,
    pub mode: FfiWriteMode,
    pub timeout_millis: u64,
    pub ensure_mode: Option<FfiEnsureMode>,
    pub data: Vec<u8>,
}

#[derive(Clone)]
pub enum FfiWriteMode {
    Cover,
    Append,
}

#[derive(Clone)]
pub enum FfiEnsureMode {
    Flush,
    SyncData,
    SyncAll,
}

impl FfiReadFile {
    pub fn new(path: String, timeout_millis: u64) -> Self {
        Self {
            path,
            timeout_millis,
        }
    }
}

impl FfiWriteFile {
    pub fn new(
        path: String,
        mode: FfiWriteMode,
        timeout_millis: u64,
        ensure_mode: Option<FfiEnsureMode>,
        data: Vec<u8>,
    ) -> Self {
        Self {
            path,
            mode,
            timeout_millis,
            ensure_mode,
            data,
        }
    }
}

impl Into<WriteMode> for FfiWriteMode {
    fn into(self) -> WriteMode {
        match self {
            FfiWriteMode::Cover => WriteMode::Cover,
            FfiWriteMode::Append => WriteMode::Append,
        }
    }
}

impl Into<EnsureMode> for FfiEnsureMode {
    fn into(self) -> EnsureMode {
        match self {
            FfiEnsureMode::Flush => EnsureMode::Flush,
            FfiEnsureMode::SyncData => EnsureMode::SyncData,
            FfiEnsureMode::SyncAll => EnsureMode::SyncAll,
        }
    }
}

impl Into<ReadFile> for FfiReadFile {
    fn into(self) -> ReadFile {
        ReadFile {
            path: self.path,
            timeout: Duration::from_millis(self.timeout_millis),
        }
    }
}

impl<'a> From<&'a FfiWriteFile> for WriteFile<'a> {
    fn from(value: &'a FfiWriteFile) -> Self {
        WriteFile {
            path: value.path.clone(),
            mode: value.clone().mode.into(),
            timeout: Duration::from_millis(value.timeout_millis),
            ensure_mode: value
                .clone()
                .ensure_mode
                .map(|ensure_mode| ensure_mode.into()),
            data: &value.data,
        }
    }
}
