use crate::domain::models::audio_models::{AudioError, AudioRecordSource};
use crate::domain::traits::audio_traits::AudioRecorderBackend;
#[cfg(target_os = "android")]
use android_media::{AudioEncoding, AudioMicrophone, ChannelInConfig, SampleRate};
use futures::Stream;
#[cfg(target_os = "android")]
use jni::JNIEnv;
#[cfg(target_os = "android")]
use jni::objects::GlobalRef;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::{Sender, channel};
use tokio::task::{AbortHandle, spawn_blocking};
use tokio_stream::wrappers::WatchStream;

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

#[cfg(target_os = "android")]
pub struct AndroidAudioRecorderBackend<'local> {
    env: RwLock<Option<JNIEnv<'local>>>,
    context: GlobalRef,
    mic: RwLock<Option<Arc<AudioMicrophone>>>,
    mic_stream: RwLock<Option<Arc<AudioMicrophoneStream>>>,
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
