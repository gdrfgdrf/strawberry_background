use crate::domain::models::storage_models::{EnsureMode, ReadFile, WriteFile, WriteMode};
use std::time::Duration;

pub struct FfiReadFile {
    pub path: String,
    pub timeout_millis: u64,
}

pub struct FfiWriteFile {
    pub path: String,
    pub mode: FfiWriteMode,
    pub timeout: u64,
    pub ensure_mode: Option<FfiEnsureMode>,
    pub data: Vec<u8>,
}

pub enum FfiWriteMode {
    Cover,
    Append,
}

pub enum FfiEnsureMode {
    Flush,
    SyncData,
    SyncAll
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
            FfiEnsureMode::SyncAll => EnsureMode::SyncAll
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

impl Into<WriteFile> for FfiWriteFile {
    fn into(self) -> WriteFile {
        WriteFile {
            path: self.path,
            mode: self.mode.into(),
            timeout: Duration::from_millis(self.timeout),
            ensure_mode: self.ensure_mode.map(|ensure_mode| ensure_mode.into()),
            data: self.data,
        }
    }
}
