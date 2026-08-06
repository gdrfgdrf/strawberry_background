use crate::domain::models::audio_models::{AudioEngineStatus, AudioError};
use crate::domain::traits::audio_traits::AudioEngine;
use crate::superstructure::audio::audio_equalizer::{ArcEqualizerSource, EqualizerSource};
use crate::utils::fft_visualiser::{FftData, run_custom_visualizer};
use crate::utils::streaming_reader::StreamingReader;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use parking_lot::{Mutex, RwLock};
use rodio::cpal::traits::HostTrait;
use rodio::{Decoder, DeviceSinkBuilder, DeviceTrait, MixerDeviceSink, Player, Source, cpal};
use rodio_tap::TapReader;
use std::io::Cursor;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync;
use tokio_stream::wrappers::{BroadcastStream, WatchStream};
use tokio_util::sync::CancellationToken;
use tracing::Level;

pub struct RodioEngine {
    handle: RwLock<Option<MixerDeviceSink>>,
    player: RwLock<Option<Arc<Player>>>,
    error_sender: Arc<sync::broadcast::Sender<AudioError>>,
    position_sender: Arc<sync::watch::Sender<Duration>>,
    status_sender: Arc<sync::watch::Sender<AudioEngineStatus>>,
    tap_reader_sender: Arc<sync::watch::Sender<Option<Arc<TapReader>>>>,
    cancellation_token: CancellationToken,
    _session: tracing::span::Span,
}

impl RodioEngine {
    pub fn new() -> Self {
        RodioEngine {
            handle: RwLock::new(None),
            player: RwLock::new(None),
            error_sender: Arc::new(sync::broadcast::channel(256).0),
            position_sender: Arc::new(sync::watch::channel(Duration::ZERO).0),
            status_sender: Arc::new(sync::watch::channel(AudioEngineStatus::Default).0),
            tap_reader_sender: Arc::new(sync::watch::channel(None).0),
            cancellation_token: CancellationToken::new(),
            _session: tracing::span!(Level::INFO, "rodio-engine"),
        }
    }

    pub fn tap_reader_stream(
        &self,
    ) -> Result<impl Stream<Item = Option<Arc<TapReader>>>, AudioError> {
        let receiver = self.tap_reader_sender.subscribe();
        Ok(WatchStream::new(receiver))
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    pub fn play_stream_with_equalizer(
        &self,
        streaming_reader: StreamingReader,
    ) -> Result<ArcEqualizerSource<Decoder<StreamingReader>>, AudioError> {
        match self.player.read().as_ref() {
            None => {
                tracing::error!("not initialized");
                Err(AudioError::NotInitialized)
            }
            Some(player) => {
                let length = streaming_reader.length();
                if length.is_none() {
                    tracing::error!("length required");
                    return Err(AudioError::LengthRequired);
                }
                let length = length.unwrap();

                player.clear();
                let source = Decoder::builder()
                    .with_seekable(true)
                    .with_byte_len(length)
                    .with_data(streaming_reader)
                    .build()?;
                let sample_rate = source.sample_rate().get();
                let equalizer_source = ArcEqualizerSource::new(source, sample_rate);
                let cloned_equalizer_source = equalizer_source.clone();

                let (reader, adapter) = TapReader::<2>::new(equalizer_source);
                player.append(adapter);
                player.play();
                let _ = self.tap_reader_sender.send(Some(reader));
                Ok(cloned_equalizer_source)
            }
        }
    }
}

impl AudioEngine for RodioEngine {
    #[tracing::instrument(skip(self), parent = &self._session)]
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
                tracing::error!(error = %original_err, "get default device error, trying fallback devices");
                let devices = match cpal::default_host().output_devices() {
                    Ok(devices) => devices,
                    Err(e) => {
                        tracing::error!(error = %e, "enumerate output devices error");
                        return Err(original_err);
                    }
                };
                let valid_devices: Vec<_> = devices
                    .filter(|dev| {
                        dev.description()
                            .map(|desc| desc.driver().is_some_and(|driver| driver != "null"))
                            .unwrap_or(false)
                    })
                    .collect();
                tracing::debug!(device_count = valid_devices.len(), "available non-null devices");
                valid_devices
                    .into_iter()
                    .find_map(|d| {
                        DeviceSinkBuilder::from_device(d)
                            .and_then(|x| x.open_sink_or_fallback())
                            .ok()
                    })
                    .ok_or_else(|| {
                        tracing::error!("no fallback device succeeded");
                        original_err
                    })
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
                let is_paused = player.is_paused();
                let is_empty = player.empty();

                if is_paused {
                    let _ = status_sender.send(AudioEngineStatus::Paused);
                } else {
                    if !is_empty {
                        let _ = status_sender.send(AudioEngineStatus::Playing);
                    } else {
                        let _ = status_sender.send(AudioEngineStatus::Finished);
                    }
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

    #[tracing::instrument(skip(self), parent = &self._session)]
    fn resume(&self) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => {
                tracing::error!("not initialized");
                Err(AudioError::NotInitialized)
            },
            Some(player) => {
                player.play();
                Ok(())
            }
        }
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    fn pause(&self) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => {
                tracing::error!("not initialized");
                Err(AudioError::NotInitialized)
            },
            Some(player) => {
                player.pause();
                Ok(())
            }
        }
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    fn seek(&self, position: Duration) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => {
                tracing::error!("not initialized");
                Err(AudioError::NotInitialized)
            },
            Some(player) => {
                tracing::debug!(?position, "seek");
                let _ = player.try_seek(position)?;
                Ok(())
            }
        }
    }

    fn get_volume(&self) -> Result<f32, AudioError> {
        match self.player.read().as_ref() {
            None => Err(AudioError::NotInitialized),
            Some(player) => Ok(player.volume()),
        }
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    fn set_volume(&self, volume: f32) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => {
                tracing::error!("not initialized");
                Err(AudioError::NotInitialized)
            },
            Some(player) => {
                tracing::debug!(?volume, "set volume");
                player.set_volume(volume);
                Ok(())
            }
        }
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    fn play_cursor(&self, cursor: Cursor<Bytes>) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => {
                tracing::error!("not initialized");
                Err(AudioError::NotInitialized)
            },
            Some(player) => {
                player.clear();
                let source = Decoder::try_from(cursor)?;
                let (reader, adapter) = TapReader::<2>::new(source);
                player.append(adapter);
                player.play();
                let _ = self.tap_reader_sender.send(Some(reader));
                Ok(())
            }
        }
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    fn play_stream(&self, streaming_reader: StreamingReader) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => {
                tracing::error!("play stream failed: not initialized");
                Err(AudioError::NotInitialized)
            },
            Some(player) => {
                let length = streaming_reader.length();
                if length.is_none() {
                    tracing::error!("length required");
                    return Err(AudioError::LengthRequired);
                }
                let length = length.unwrap();

                player.clear();
                let source = Decoder::builder()
                    .with_seekable(true)
                    .with_byte_len(length)
                    .with_data(streaming_reader)
                    .build()?;
                let (reader, adapter) = TapReader::<2>::new(source);
                player.append(adapter);
                player.play();
                let _ = self.tap_reader_sender.send(Some(reader));
                Ok(())
            }
        }
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    fn reset(&self) -> Result<(), AudioError> {
        match self.player.read().as_ref() {
            None => {
                tracing::error!("not initialized");
                Err(AudioError::NotInitialized)
            },
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
    use futures_util::StreamExt;
    use parking_lot::Condvar;
    use parking_lot::lock_api::Mutex;
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::thread;
    use tokio::runtime::Runtime;

    macro_rules! await_test {
        ($e:expr) => {
            tokio_test::block_on($e)
        };
    }

    #[test]
    fn test_audio_player() {
        let runtime = Arc::new(Runtime::new().unwrap());
        let engine = RodioEngine::new();
        engine.init().unwrap();

        let shared_buffer = Arc::new(SharedBuffer {
            data: Mutex::new(Vec::with_capacity(1024 * 64)),
            eof: Mutex::new(false),
            condvar: Condvar::new(),
            length: Mutex::new(None),
        });
        let cloned_shared_buffer = shared_buffer.clone();
        thread::spawn(move || {
            let url = "https://samplelib.com/mp3/sample-10s.mp3";
            let client = reqwest::blocking::Client::new();
            let mut response = match client.get(url).send() {
                Ok(r) => r,
                Err(_) => {
                    *cloned_shared_buffer.eof.lock() = true;
                    cloned_shared_buffer.condvar.notify_one();
                    return;
                }
            };
            {
                let length = response.content_length();
                *cloned_shared_buffer.length.lock() = length;
            }

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

        // let mut file = OpenOptions::new()
        //     .create(true)
        //     .append(true)
        //     .open("fft_visualiser.txt")
        //     .unwrap();
        //
        // while let Some(fft_data) = await_test!(engine.fft_stream().unwrap().next()) {
        //     let channel0 = fft_data.channel_datas.first();
        //     if let Some(channel0) = channel0 {
        //         let formatted = format!(
        //             "[{} Hz], channel 0: {:?}, {:?}, {:?}, {:?}\n",
        //             fft_data.sample_rate,
        //             fft_data.channel_datas.first().unwrap().datas.first(),
        //             fft_data.channel_datas.first().unwrap().datas.get(1),
        //             fft_data.channel_datas.first().unwrap().datas.get(2),
        //             fft_data.channel_datas.first().unwrap().datas.get(3)
        //         );
        //         let _ = file.write(&*formatted.into_bytes());
        //         let _ = file.sync_all();
        //     }
        // }
    }
}
