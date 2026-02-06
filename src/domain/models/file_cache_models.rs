use rkyv::{Archive, Deserialize, Serialize, bytecheck::CheckBytes};

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, CheckBytes)]
pub struct CacheChannel {
    pub name: String,
    pub extension: Option<String>,
    pub records: Vec<CacheRecord>,
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, CheckBytes, Clone)]
pub struct CacheRecord {
    pub tag: String,
    pub filename: String,
    pub size: usize,
    pub sentence: String
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("IO Error: {0}")]
    IO(String),
    #[error("File {0} does not exist")]
    FileNotExist(String),
    #[error("Tag {0} does not exist")]
    TagNotExist(String),
    #[error("Cache Manager {0} does not exist")]
    ManagerNotExist(String),
    #[error("An locking error occurs when accessing {0}")]
    Lock(String),
    #[error("Serialize Error: {0}")]
    Serialization(String),
    #[error("Timeout: {0}")]
    Timeout(String),
}