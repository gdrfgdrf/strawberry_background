use crate::domain::models::audio_models::{AudioEngineStatus, AudioError};
use crate::domain::traits::audio_traits::AudioEngine;
use crate::utils::streaming_reader::StreamingReader;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use parking_lot::{Mutex, RwLock};
use rodio::cpal::traits::HostTrait;
use rodio::{Decoder, DeviceSinkBuilder, DeviceTrait, MixerDeviceSink, Player, cpal};
use rodio_tap::{ChannelSpectrum, TapReader, Transform, Visualizer, VisualizerConfig};
use std::io::Cursor;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, WatchStream};
use tokio_util::sync::CancellationToken;

pub struct FftChannelData {
    pub datas: Vec<f32>,
}

pub struct FftData {
    pub sample_rate: u32,
    pub channel_datas: Vec<FftChannelData>,
}

pub struct RodioEngine {
    tokio_runtime: Arc<Runtime>,
    handle: RwLock<Option<MixerDeviceSink>>,
    player: RwLock<Option<Arc<Player>>>,
    error_sender: Arc<sync::broadcast::Sender<AudioError>>,
    position_sender: Arc<sync::watch::Sender<Duration>>,
    status_sender: Arc<sync::watch::Sender<AudioEngineStatus>>,
    fft_sender: Arc<sync::watch::Sender<FftData>>,
    fft_thread_handle: Mutex<Option<JoinHandle<()>>>,
    cancellation_token: CancellationToken,
}

impl RodioEngine {
    pub fn new(tokio_runtime: Arc<Runtime>) -> Self {
        RodioEngine {
            tokio_runtime,
            handle: RwLock::new(None),
            player: RwLock::new(None),
            error_sender: Arc::new(sync::broadcast::channel(256).0),
            position_sender: Arc::new(sync::watch::channel(Duration::ZERO).0),
            status_sender: Arc::new(sync::watch::channel(AudioEngineStatus::Default).0),
            fft_sender: Arc::new(sync::watch::channel(FftData::empty()).0),
            fft_thread_handle: Mutex::new(None),
            cancellation_token: CancellationToken::new(),
        }
    }

    fn start_fft(&self, reader: Arc<TapReader>) {
        let mut handle = self.fft_thread_handle.lock();
        if let Some(handle) = handle.take() {
            handle.abort();
        }

        let cloned_sender = self.fft_sender.clone();
        *handle = Some(self.tokio_runtime.spawn(async move {
            let config = VisualizerConfig {
                period: Duration::from_millis(10),
                transform: Transform::FourierLog(28),
                ..Default::default()
            };
            Visualizer::<2>::run_with_frame_reader(
                move || Some(Arc::clone(&reader)),
                config,
                move |channels, sample_rate_hz| {
                    let fft_data = FftData::new(channels, sample_rate_hz);
                    let _ = cloned_sender.send(fft_data);
                },
            );
        }));
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
                let (reader, adapter) = TapReader::<2>::new(source);
                player.append(adapter);
                player.play();
                self.start_fft(reader);
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
                let (reader, adapter) = TapReader::<2>::new(source);
                player.append(adapter);
                player.play();
                self.start_fft(reader);
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

impl FftData {
    fn new(channels: &[ChannelSpectrum], sample_rate: u32) -> Self {
        let mut channel_datas = Vec::<FftChannelData>::new();
        channels.iter().for_each(|channel| {
            let mut channel_data = Vec::<f32>::new();
            for (_, magnitude) in channel.bins.iter().copied().take(5).enumerate() {
                channel_data.push(magnitude);
            }
            channel_datas.push(FftChannelData {
                datas: channel_data,
            });
        });

        FftData {
            sample_rate,
            channel_datas,
        }
    }

    fn empty() -> Self {
        FftData {
            sample_rate: 0,
            channel_datas: Vec::new(),
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
    use tokio::runtime::Runtime;

    #[test]
    fn test_audio_player() {
        let runtime = Arc::new(Runtime::new().unwrap());
        let engine = RodioEngine::new(runtime);
        engine.init().unwrap();

        let shared_buffer = Arc::new(SharedBuffer {
            data: Mutex::new(Vec::with_capacity(1024 * 64)),
            eof: Mutex::new(false),
            condvar: Condvar::new(),
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

        sleep(Duration::from_secs(15))
    }
}
