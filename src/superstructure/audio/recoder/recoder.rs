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
use futures_util::StreamExt;
#[cfg(target_os = "android")]
use jni::JNIEnv;
#[cfg(target_os = "android")]
use jni::objects::GlobalRef;
use parking_lot::{Mutex, RwLock};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::task::{Context, Poll, Waker};
#[cfg(target_os = "android")]
use std::time::Duration;
use tokio::sync::mpsc;
#[cfg(target_os = "android")]
use tokio::task::AbortHandle;
use tokio::task::spawn_blocking;

struct RecordingStateInner {
    paused: AtomicBool,
    stopped: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

enum RecorderState {
    Idle,
    Recording(Arc<RecordingStateInner>),
}

pub struct AudioRecorder<T: AudioRecorderBackend> {
    backend: T,
    state: Arc<Mutex<RecorderState>>,
    disposed: AtomicBool,
}

impl<T: AudioRecorderBackend> AudioRecorder<T> {
    pub fn new(backend: T) -> Self {
        Self {
            backend,
            state: Arc::new(Mutex::new(RecorderState::Idle)),
            disposed: AtomicBool::new(false),
        }
    }

    pub fn start(
        &self,
        source: AudioRecordSource,
        sample_rate: Option<u32>,
        channels: Option<u16>,
        sample_size: Option<u32>,
    ) -> Result<impl Stream<Item = Vec<f32>>, AudioError> {
        if self.disposed.load(Ordering::Acquire) {
            return Err(AudioError::RecorderDisposed);
        }

        let mut guard = self.state.lock();
        match &*guard {
            RecorderState::Idle => {
                let inner_stream =
                    self.backend
                        .start(source, sample_rate, channels, sample_size)?;
                let recording_inner = Arc::new(RecordingStateInner {
                    paused: AtomicBool::new(false),
                    stopped: AtomicBool::new(false),
                    waker: Mutex::new(None),
                });
                *guard = RecorderState::Recording(recording_inner.clone());
                Ok(RecordingStream {
                    inner: Box::pin(inner_stream),
                    state_inner: recording_inner,
                    recorder_state: Arc::downgrade(&self.state),
                })
            }
            _ => Err(AudioError::AlreadyRecording),
        }
    }

    pub fn pause(&self) -> Result<(), AudioError> {
        self.ensure_active()?;
        let guard = self.state.lock();
        if let RecorderState::Recording(ref inner) = *guard {
            inner.paused.store(true, Ordering::Release);
            return Ok(());
        }
        Err(AudioError::NotRecording)
    }

    pub fn resume(&self) -> Result<(), AudioError> {
        self.ensure_active()?;
        let guard = self.state.lock();
        if let RecorderState::Recording(ref inner) = *guard {
            inner.paused.store(false, Ordering::Release);
            if let Some(waker) = inner.waker.lock().take() {
                waker.wake();
            }
            Ok(())
        } else {
            Err(AudioError::NotRecording)
        }
    }

    pub fn stop(&self) -> Result<(), AudioError> {
        self.ensure_active()?;
        let mut guard = self.state.lock();
        let old_state = std::mem::replace(&mut *guard, RecorderState::Idle);
        match old_state {
            RecorderState::Recording(inner) => {
                inner.stopped.store(true, Ordering::Release);
                if let Some(waker) = inner.waker.lock().take() {
                    waker.wake();
                }
                Ok(())
            }
            _ => Err(AudioError::NotRecording),
        }
    }

    pub fn is_recording(&self) -> bool {
        matches!(*self.state.lock(), RecorderState::Recording(_))
    }

    pub fn is_paused(&self) -> bool {
        if let RecorderState::Recording(ref inner) = *self.state.lock() {
            return inner.paused.load(Ordering::Acquire);
        }
        false
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed.load(Ordering::Acquire)
    }

    pub fn dispose(&self) -> Result<(), AudioError> {
        if self.disposed.swap(true, Ordering::AcqRel) {
            return Err(AudioError::RecorderDisposed);
        }
        let _ = self.stop();
        self.backend.dispose()
    }

    fn ensure_active(&self) -> Result<(), AudioError> {
        if self.disposed.load(Ordering::Acquire) {
            return Err(AudioError::RecorderDisposed);
        }
        Ok(())
    }
}

struct RecordingStream<S> {
    inner: S,
    state_inner: Arc<RecordingStateInner>,
    recorder_state: Weak<Mutex<RecorderState>>,
}

impl<S: Stream<Item = Vec<f32>> + Unpin> Stream for RecordingStream<S> {
    type Item = Vec<f32>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.state_inner.stopped.load(Ordering::Acquire) {
            return Poll::Ready(None);
        }

        if this.state_inner.paused.load(Ordering::Acquire) {
            *this.state_inner.waker.lock() = Some(context.waker().clone());
            return Poll::Pending;
        }

        this.inner.poll_next_unpin(context)
    }
}

impl<S> Drop for RecordingStream<S> {
    fn drop(&mut self) {
        self.state_inner.stopped.store(true, Ordering::Release);
        if let Some(waker) = self.state_inner.waker.lock().take() {
            waker.wake();
        }

        if let Some(state_arc) = self.recorder_state.upgrade() {
            let mut guard = state_arc.lock();
            if matches!(
                &*guard,
                RecorderState::Recording(inner) if Arc::ptr_eq(inner, &self.state_inner)
            ) {
                *guard = RecorderState::Idle;
            }
        }
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
            recoder: RwLock::new(Some(Recorder::new())),
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
    fn start(
        &self,
        _: AudioRecordSource,
        sample_rate: Option<u32>,
        channels: Option<u16>,
        sample_size: Option<u32>,
    ) -> Result<impl Stream<Item = Vec<f32>>, AudioError> {
        let mut recorder = self.recoder.write();
        if recorder.is_none() {
            return Err(AudioError::NotInitialized);
        }
        let recorder = recorder.as_mut().unwrap();
        let receiver = recorder
            .start(false, sample_rate, channels, sample_size)
            .map_err(|e| AudioError::ErrorForward(e.to_string()))?;
        Ok(Self::crossbeam_to_stream(receiver, 300))
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
            mic_stream: RwLock::new(None),
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