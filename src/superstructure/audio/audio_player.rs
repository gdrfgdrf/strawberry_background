use crate::domain::models::audio_models::{AudioEngineStatus, AudioError};
use crate::domain::traits::audio_traits::AudioEngine;
use crate::utils::streaming_reader::StreamingReader;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use parking_lot::RwLock;
use rodio::cpal::traits::HostTrait;
use rodio::{Decoder, DeviceSinkBuilder, DeviceTrait, MixerDeviceSink, Player, cpal};
use std::io::Cursor;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use tokio::sync;
use tokio_stream::wrappers::{BroadcastStream, WatchStream};
use tokio_util::sync::CancellationToken;

pub struct RodioEngine {
    handle: RwLock<Option<MixerDeviceSink>>,
    player: RwLock<Option<Arc<Player>>>,
    error_sender: Arc<sync::broadcast::Sender<AudioError>>,
    position_sender: Arc<sync::watch::Sender<Duration>>,
    status_sender: Arc<sync::watch::Sender<AudioEngineStatus>>,
    cancellation_token: CancellationToken,
}

impl RodioEngine {
    pub fn new() -> Self {
        RodioEngine {
            handle: RwLock::new(None),
            player: RwLock::new(None),
            error_sender: Arc::new(sync::broadcast::channel(256).0),
            position_sender: Arc::new(sync::watch::channel(Duration::ZERO).0),
            status_sender: Arc::new(sync::watch::channel(AudioEngineStatus::Default).0),
            cancellation_token: CancellationToken::new(),
        }
    }
}

impl AudioEngine for RodioEngine {
    fn init(&self) -> Result<(), AudioError> {
        let error_sender = self.error_sender.clone();
        let position_sender = self.position_sender.clone();
        let status_sender = self.status_sender.clone();

        let handle = DeviceSinkBuilder::from_default_device()
            .and_then(|x| {
                x.with_error_callback(move |error| {
                    let _ = error_sender.send(AudioError::ErrorForward(error.to_string()));
                })
                .open_stream()
            })
            .or_else(|original_err| {
                let devices = match cpal::default_host().output_devices() {
                    Ok(devices) => devices,
                    Err(_) => {
                        return Err(original_err);
                    }
                };
                devices
                    .filter(|dev| {
                        dev.description()
                            .map(|desc| desc.driver().is_some_and(|driver| driver != "null"))
                            .unwrap_or(false)
                    })
                    .find_map(|d| {
                        DeviceSinkBuilder::from_device(d)
                            .and_then(|x| x.open_sink_or_fallback())
                            .ok()
                    })
                    .ok_or(original_err)
            })?;
        let player = Arc::new(Player::connect_new(handle.mixer()));
        let cloned_player = player.clone();
        let cloned_cancellation_token = self.cancellation_token.clone();
        std::thread::spawn(move || {
            loop {
                if cloned_cancellation_token.is_cancelled() {
                    return;
                }

                let position = player.get_pos();
                let _ = position_sender.send(position);

                if player.is_paused() || player.empty() {
                    let _ = status_sender.send(AudioEngineStatus::Paused);
                } else {
                    let _ = status_sender.send(AudioEngineStatus::Playing);
                }
                sleep(Duration::from_millis(10))
            }
        });

        *self.handle.write() = Some(handle);
        *self.player.write() = Some(cloned_player);

        Ok(())
    }

    fn error_stream(&self) -> Result<impl Stream<Item = AudioError>, AudioError> {
        let receiver = self.error_sender.subscribe();
        Ok(BroadcastStream::new(receiver).filter_map(|result| async move { result.ok() }))
    }

    fn position_stream(&self) -> Result<impl Stream<Item = Duration>, AudioError> {
        let receiver = self.position_sender.subscribe();
        Ok(WatchStream::new(receiver))
    }

    fn status_stream(&self) -> Result<impl Stream<Item = AudioEngineStatus>, AudioError> {
        let receiver = self.status_sender.subscribe();
        Ok(WatchStream::new(receiver))
    }

    fn resume(&self) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => Err(AudioError::NotInitialized),
            Some(player) => {
                player.play();
                Ok(())
            }
        }
    }

    fn pause(&self) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => Err(AudioError::NotInitialized),
            Some(player) => {
                player.pause();
                Ok(())
            }
        }
    }

    fn seek(&self, position: Duration) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => Err(AudioError::NotInitialized),
            Some(player) => {
                let _ = player.try_seek(position)?;
                Ok(())
            }
        }
    }

    fn play_cursor(&self, cursor: Cursor<Bytes>) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => Err(AudioError::NotInitialized),
            Some(player) => {
                player.clear();
                let source = Decoder::try_from(cursor)?;
                player.append(source);
                player.play();
                Ok(())
            }
        }
    }

    fn play_stream(&self, streaming_reader: StreamingReader) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => Err(AudioError::NotInitialized),
            Some(player) => {
                player.clear();
                let source = Decoder::new(streaming_reader)?;
                player.append(source);
                player.play();
                Ok(())
            }
        }
    }

    fn reset(&self) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => Err(AudioError::NotInitialized),
            Some(player) => {
                player.clear();
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::traits::audio_traits::AudioEngine;
    use crate::superstructure::audio::audio_player::RodioEngine;
    use crate::utils::streaming_reader::{SharedBuffer, StreamingReader};
    use parking_lot::Condvar;
    use parking_lot::lock_api::Mutex;
    use std::io::Read;
    use std::sync::Arc;
    use std::thread;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_audio_player() {
        let engine = RodioEngine::new();
        engine.init().unwrap();

        let shared_buffer = Arc::new(SharedBuffer {
            data: Mutex::new(Vec::with_capacity(1024 * 64)),
            eof: Mutex::new(false),
            condvar: Condvar::new(),
        });
        let cloned_shared_buffer = shared_buffer.clone();
        thread::spawn(move || {
            let url = "https://samplelib.com/mp3/sample-speech-30m.mp3";
            let client = reqwest::blocking::Client::new();
            let mut response = match client.get(url).send() {
                Ok(r) => r,
                Err(_) => {
                    *cloned_shared_buffer.eof.lock() = true;
                    cloned_shared_buffer.condvar.notify_one();
                    return;
                }
            };

            let mut buf = vec![0u8; 8192];
            loop {
                match response.read(&mut buf) {
                    Ok(0) => {
                        *cloned_shared_buffer.eof.lock() = true;
                        cloned_shared_buffer.condvar.notify_one();
                        break;
                    }
                    Ok(n) => {
                        cloned_shared_buffer
                            .data
                            .lock()
                            .extend_from_slice(&buf[..n]);
                        cloned_shared_buffer.condvar.notify_one();
                    }
                    Err(_) => {
                        *cloned_shared_buffer.eof.lock() = true;
                        cloned_shared_buffer.condvar.notify_one();
                        break;
                    }
                }
            }
        });

        let mut reader = StreamingReader::new(shared_buffer);
        reader.wait_for_data(8192).unwrap();
        engine.play_stream(reader).unwrap();

        sleep(Duration::from_secs(60 * 8))
    }
}
