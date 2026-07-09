

#[derive(Debug, Clone, thiserror::Error)]
pub enum AudioError {
    #[error("Error Forward: {0}")]
    ErrorForward(String),
    #[error("Not initialized")]
    NotInitialized,
    #[error("Length is required")]
    LengthRequired
}

#[derive(Clone)]
pub enum AudioEngineStatus {
    Default,
    Paused,
    Finished,
    Playing,
}