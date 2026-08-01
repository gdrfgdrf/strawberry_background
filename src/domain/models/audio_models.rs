

#[derive(Debug, Clone, thiserror::Error)]
pub enum AudioError {
    #[error("Error Forward: {0}")]
    ErrorForward(String),
    #[error("Not initialized")]
    NotInitialized,
    #[error("Length is required")]
    LengthRequired,
    #[error("Platform mismatch")]
    PlatformMismatch,
    #[error("JNI Environment Required")]
    JNIEnvironmentRequired,
    #[error("No default output device")]
    NoDefaultOutputDevice,
    #[error("Unsupported")]
    Unsupported
}

#[derive(Clone)]
pub enum AudioEngineStatus {
    Default,
    Paused,
    Finished,
    Playing,
}

#[derive(Clone)]
pub enum AudioRecordSource {
    Mic,
    Device
}