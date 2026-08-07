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
    #[error("Rkv Not Initialized")]
    RkvNotInitialized,
    #[error("IO Error: {0}")]
    IO(String),
    #[error("File Not Submitted: {0}")]
    FileNotSubmitted(String),
    #[error("Channel Not Exists: {0}")]
    ChannelNotExists(String),
    #[error("Record Not Exists: {0}")]
    RecordNotExists(String),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Error Forwarding: {0}")]
    ErrorForward(String)
}