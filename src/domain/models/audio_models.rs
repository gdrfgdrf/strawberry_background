


pub struct RawAudioData<'a> {
    pub channels: Vec<AudioChannelData<'a>>
}

pub struct AudioChannelData<'a> {
    pub index: i32,
    pub data: &'a Vec<u8>
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    
}

pub enum AudioEngineStatus {
    Initializing,
    Ready,
    Playing,
    Paused,
    Stopped,
}