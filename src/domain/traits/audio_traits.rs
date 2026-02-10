use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use crate::domain::models::audio_models::{AudioEngineStatus, AudioError, RawAudioData};

#[async_trait]
trait AudioSource: Send + Sync + 'static {
    async fn identifier(&self) -> String;
    async fn get_data(&self) -> Result<Arc<RawAudioData>, AudioError>;
}

#[async_trait]
trait PlaylistManager: Send + Sync + 'static {
    async fn has_sources(&self) -> bool;
    async fn set_sources(&self, sources: Vec<Arc<dyn AudioSource>>);
    async fn remove_sources(&self) -> Option<Vec<Arc<dyn AudioSource>>>;
    async fn get_sources(&self) -> Option<Vec<Arc<dyn AudioSource>>>;
    
    async fn has_source(&self, identifier: &String) -> bool;
    async fn add_source(&self, source: Arc<dyn AudioSource>);
    async fn remove_source(&self, identifier: &String) -> Option<Arc<dyn AudioSource>>;
    async fn get_source(&self, identifier: &String) -> Option<Arc<dyn AudioSource>>;
    
    async fn get_identifiers(&self) -> Option<Vec<String>>;
}

#[async_trait]
trait AudioSourceShuffler: Send + Sync + 'static {
    async fn shuffle(&self, identifiers: &Vec<String>) -> Vec<String>;
}

#[async_trait]
trait AudioEngine: Send + Sync + 'static {
    async fn has_shuffler(&self) -> bool;
    async fn set_shuffler(&self, shuffler: Arc<dyn AudioSourceShuffler>);
    async fn remove_shuffler(&self) -> Option<Arc<dyn AudioSourceShuffler>>;
    async fn get_shuffler(&self) -> Option<Arc<dyn AudioSourceShuffler>>;
    
    async fn has_playlist_manager(&self) -> bool;
    async fn set_playlist_manager(&self, manager: Arc<dyn PlaylistManager>);
    async fn remove_playlist_manager(&self) -> Option<Arc<dyn PlaylistManager>>;
    async fn get_playlist_manager(&self) -> Option<Arc<dyn PlaylistManager>>;
    
    async fn has_previous(&self) -> bool;
    async fn previous_source(&self) -> Result<Arc<dyn AudioSource>, AudioError>;
    async fn has_current(&self) -> bool;
    async fn current_source(&self) -> Result<Arc<dyn AudioSource>, AudioError>;
    async fn current_source_index(&self) -> Result<i32, AudioError>;
    async fn has_next(&self) -> bool;
    async fn next_source(&self) -> Result<Arc<dyn AudioSource>, AudioError>;

    async fn play(&self) -> Result<(), AudioError>;
    async fn play_by_identifier(&self, identifier: &String) -> Result<(), AudioError>;
    async fn play_by_index(&self, index: i32) -> Result<(), AudioError>;
    async fn play_previous(&self) -> Result<(), AudioError>;
    async fn play_next(&self) -> Result<(), AudioError>;
    
    async fn seek_to(&self, duration: Duration) -> Result<(), AudioError>;
    
    async fn pause(&self) -> Result<(), AudioError>;
    async fn stop(&self) -> Result<(), AudioError>;
    
    async fn current_position(&self) -> Result<Duration, AudioError>;
    async fn total_duration(&self) -> Result<Duration, AudioError>;
    
    async fn status(&self) -> AudioEngineStatus;
}