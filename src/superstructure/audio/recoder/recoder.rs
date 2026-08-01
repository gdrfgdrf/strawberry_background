use crate::domain::models::audio_models::{AudioError, AudioRecordSource};
use crate::domain::traits::audio_traits::AudioRecorderBackend;
#[cfg(target_os = "android")]
use android_media::{AudioEncoding, AudioMicrophone, ChannelInConfig, SampleRate};
#[cfg(target_os = "windows")]
use audio_recorder_rs::Recorder;
use cpal::Device;
use cpal::traits::HostTrait;
use crossbeam_channel::Receiver;
use futures::Stream;
#[cfg(target_os = "android")]
use jni::JNIEnv;
#[cfg(target_os = "android")]
use jni::objects::GlobalRef;
use parking_lot::RwLock;
#[cfg(target_os = "android")]
use std::sync::Arc;
#[cfg(target_os = "android")]
use std::time::Duration;
use tokio::sync::mpsc;
#[cfg(target_os = "android")]
use tokio::task::AbortHandle;
use tokio::task::spawn_blocking;

pub struct AudioRecorder<T: AudioRecorderBackend> {
    backend: T,
}

impl<T: AudioRecorderBackend> AudioRecorder<T> {
    pub fn new(backend: T) -> Self {
        Self { backend }
    }

    pub fn start(
        &self,
        source: AudioRecordSource,
    ) -> Result<impl Stream<Item = Vec<f32>>, AudioError> {
        self.backend.start(source)
    }

    pub fn dispose(&self) -> Result<(), AudioError> {
        self.backend.dispose()
    }
}

#[cfg(target_os = "windows")]
pub struct DesktopAudioRecorderBackend {
    recoder: RwLock<Option<Recorder>>,
}

#[cfg(target_os = "windows")]
impl DesktopAudioRecorderBackend {
    pub fn new() -> DesktopAudioRecorderBackend {
        DesktopAudioRecorderBackend {
            recoder: RwLock::new(Some(Recorder::new()))
        }
    }

    pub fn crossbeam_to_stream<T: Send + 'static>(
        rx: Receiver<T>,
        buffer: usize,
    ) -> impl Stream<Item = T> {
        let (tx, stream_rx) = mpsc::channel(buffer);

        spawn_blocking(move || {
            while let Ok(item) = rx.recv() {
                if tx.blocking_send(item).is_err() {
                    break;
                }
            }
        });

        tokio_stream::wrappers::ReceiverStream::new(stream_rx)
    }

    /// copied from audio_recorder_rs's get_default_device.rs
    pub fn get_default_output_device() -> Result<Device, AudioError> {
        #[cfg(target_os = "macos")]
        {
            if let Ok(host) = cpal::host_from_id(cpal::HostId::ScreenCaptureKit) {
                if let Some(device) = host.default_input_device() {
                    return Ok(device);
                }
            }
            let host = cpal::default_host();
            let device = match host.default_output_device() {
                Some(d) => d,
                None => {
                    return Err(AudioError::NoDefaultOutputDevice);
                }
            };

            return Ok(device);
        }

        #[cfg(target_os = "windows")]
        {
            if let Ok(wasapi_host) = cpal::host_from_id(cpal::HostId::Wasapi) {
                if let Some(device) = wasapi_host.default_output_device() {
                    return Ok(device);
                }
            }

            let host = cpal::default_host();
            let device = match host.default_output_device() {
                Some(d) => d,
                None => {
                    return Err(AudioError::NoDefaultOutputDevice);
                }
            };

            Ok(device)
        }

        #[cfg(target_os = "linux")]
        {
            let host = cpal::default_host();
            let device = match host.default_output_device() {
                Some(d) => d,
                None => {
                    return Err(AudioError::NoDefaultOutputDevice);
                }
            };

            Ok(device)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let host = cpal::default_host();
            let device = match host.default_output_device() {
                Some(d) => d,
                None => {
                    return Err(AudioError::NoDefaultOutputDevice);
                }
            };

            Ok(device)
        }
    }
}

#[cfg(target_os = "windows")]
impl AudioRecorderBackend for DesktopAudioRecorderBackend {
    fn start(&self, source: AudioRecordSource) -> Result<impl Stream<Item = Vec<f32>>, AudioError> {
        let mut recorder = self.recoder.write();
        if recorder.is_none() {
            return Err(AudioError::NotInitialized);
        }
        let recorder = recorder.as_mut().unwrap();
        match source {
            AudioRecordSource::Mic => {
                let receiver = recorder
                    .start(true)
                    .map_err(|e| AudioError::ErrorForward(e.to_string()))?;
                Ok(Self::crossbeam_to_stream(receiver, 300))
            }
            AudioRecordSource::Device => {
                let output_device = Self::get_default_output_device()?;
                let receiver = recorder
                    .record_single_device(output_device)
                    .map_err(|e| AudioError::ErrorForward(e.to_string()))?;
                Ok(Self::crossbeam_to_stream(receiver, 300))
            }
        }
    }

    fn dispose(&self) -> Result<(), AudioError> {
        let mut recorder = self.recoder.write();
        if recorder.is_some() {
            recorder.take().unwrap().stop();
        }
        Ok(())
    }
}

#[cfg(target_os = "android")]
pub struct AndroidAudioRecorderBackend<'local> {
    env: RwLock<Option<JNIEnv<'local>>>,
    context: GlobalRef,
    mic: RwLock<Option<Arc<AudioMicrophone>>>,
    mic_stream: RwLock<Option<Arc<AudioMicrophoneStream>>>,
}

#[cfg(target_os = "android")]
impl<'local> AndroidAudioRecorderBackend<'local> {
    pub fn new(env: JNIEnv<'local>, context: GlobalRef) -> AndroidAudioRecorderBackend {
        Self {
            env: RwLock::new(Some(env)),
            context,
            mic: RwLock::new(None),
            mic_stream: RwLock::new(None)
        }
    }

}

#[cfg(target_os = "android")]
impl<'local> AudioRecorderBackend for AndroidAudioRecorderBackend<'local> {
    fn start(&self, source: AudioRecordSource) -> Result<impl Stream<Item = Vec<f32>>, AudioError> {
        match source {
            AudioRecordSource::Mic => {
                let mut env = self.env.write();
                if env.is_none() {
                    return Err(AudioError::JNIEnvironmentRequired);
                }

                let mic = Arc::new(AudioMicrophone::new(
                    env.take().unwrap(),
                    &self.context,
                    SampleRate::Rate44100,
                    ChannelInConfig::Stereo,
                    AudioEncoding::PcmFloat,
                )?);
                mic.start()?;

                let cloned_mic = mic.clone();
                let stream = Arc::new(AudioMicrophoneStream::new(
                    cloned_mic,
                    Duration::from_millis(100),
                    |raw| {
                        raw.chunks_exact(4)
                            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                            .collect()
                    },
                ));
                {
                    *self.mic.write() = Some(mic);
                    *self.mic_stream.write() = Some(stream);
                }
                let stream = self.mic_stream.read();
                let stream = stream.as_ref().unwrap().clone();
                let receiver = stream.subscribe()?;

                Ok(receiver)
            }
            AudioRecordSource::Device => Err(AudioError::Unsupported),
        }
    }

    fn dispose(&self) -> Result<(), AudioError> {
        let mut mic = self.mic.write();
        let mut mic_stream = self.mic_stream.write();
        if mic.is_some() {
            let mic = mic.take().unwrap();
            mic.stop()?;
        }
        if mic_stream.is_some() {
            let _ = mic_stream.take().unwrap();
        }

        Ok(())
    }
}

#[cfg(target_os = "android")]
pub struct AudioMicrophoneStream {
    sender: Sender<Vec<f32>>,
    abort_handle: AbortHandle,
}

#[cfg(target_os = "android")]
impl AudioMicrophoneStream {
    pub fn new(
        mic: Arc<AudioMicrophone>,
        chunk_duration: Duration,
        convert_fn: impl Fn(&[u8]) -> Vec<f32> + Send + Sync + 'static,
    ) -> Self {
        let (sender, _) = channel(Vec::new());
        let cloned_sender = sender.clone();

        let abort_handle = spawn_blocking(move || async move {
            loop {
                match mic.read(chunk_duration.as_millis() as i32).await {
                    Ok(raw_data) => {
                        let float_data = convert_fn(&raw_data);
                        if cloned_sender.send(float_data).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        })
        .abort_handle();

        Self {
            sender,
            abort_handle,
        }
    }

    pub fn subscribe(self: Arc<Self>) -> Result<impl Stream<Item = Vec<f32>>, AudioError> {
        let receiver = self.sender.subscribe();
        Ok(WatchStream::new(receiver))
    }
}

#[cfg(target_os = "android")]
impl Drop for AudioMicrophoneStream {
    fn drop(&mut self) {
        self.abort_handle.abort();
    }
}
