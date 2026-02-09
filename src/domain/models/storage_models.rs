use std::sync::Arc;
use std::time::Duration;

pub struct ReadFile {
    pub path: String,
    pub timeout: Duration,
}

pub struct WriteFile<'a> {
    pub path: String,
    pub mode: WriteMode,
    pub timeout: Duration,
    pub ensure_mode: Option<EnsureMode>,
    pub data: &'a Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("{0} is not a file")]
    FileRequired(String),
    #[error("{0} is not a directory")]
    DirectoryRequired(String),
    #[error("{0} does not exist")]
    NotExist(String),
    #[error("IO Error: {0}")]
    IOError(String),
    #[error("Timeout: {0}")]
    Timeout(String),
}

#[derive(Debug, Eq, PartialEq)]
pub enum WriteMode {
    Cover,
    Append,
}

#[derive(Debug, Eq, PartialEq)]
pub enum EnsureMode {
    Flush,
    SyncData,
    SyncAll
}

impl ReadFile {
    pub fn path(path: String) -> Self {
        Self {
            path,
            timeout: Duration::from_secs(60),
        }
    }
}

impl<'a> WriteFile<'a> {
    pub fn path(path: String, data: &'a Vec<u8>) -> Self {
        Self {
            path,
            mode: WriteMode::Cover,
            timeout: Duration::from_secs(60),
            ensure_mode: Some(EnsureMode::Flush),
            data,
        }
    }
}
