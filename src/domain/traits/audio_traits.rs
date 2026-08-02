use crate::domain::models::audio_models::{AudioEngineStatus, AudioError, AudioRecordSource};
use crate::utils::streaming_reader::StreamingReader;
use bytes::Bytes;
use futures_util::Stream;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync;

pub trait AudioSource {
    fn init(&self) -> Result<(), AudioError>;
    fn get_stream_reader(&self) -> Result<&StreamingReader, AudioError>;
}

pub trait AudioEngine {
    fn init(&self) -> Result<(), AudioError>;

    fn error_stream(&self) -> Result<impl Stream<Item = AudioError>, AudioError>;
    fn position_stream(&self) -> Result<impl Stream<Item = Duration>, AudioError>;
    fn status_stream(&self) -> Result<impl Stream<Item = AudioEngineStatus>, AudioError>;

    fn resume(&self) -> Result<(), AudioError>;
    fn pause(&self) -> Result<(), AudioError>;
    fn seek(&self, position: Duration) -> Result<(), AudioError>;

    fn get_volume(&self) -> Result<f32, AudioError>;
    fn set_volume(&self, volume: f32) -> Result<(), AudioError>;

    fn play_cursor(&self, cursor: Cursor<Bytes>) -> Result<(), AudioError>;
    fn play_stream(&self, streaming_reader: StreamingReader) -> Result<(), AudioError>;

    fn reset(&self) -> Result<(), AudioError>;
}

pub trait AudioRecorderBackend {
    fn start(
        &self,
        source: AudioRecordSource,
        sample_rate: Option<u32>,
        channels: Option<u16>,
        sample_size: Option<u32>,
    ) -> Result<impl Stream<Item = Vec<f32>>, AudioError>;
    fn dispose(&self) -> Result<(), AudioError>;
}
