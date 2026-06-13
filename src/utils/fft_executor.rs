use crate::domain::models::audio_models::AudioError;
use crate::utils::fft_visualiser::{FftData, run_custom_visualizer};
use futures::Stream;
use parking_lot::Mutex;
use rodio_tap::TapReader;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::WatchStream;

pub struct FftExecutor {
    tokio_runtime: Arc<Runtime>,
    num_bands: usize,
    tap_reader: Arc<TapReader>,
    fft_sender: Arc<sync::watch::Sender<FftData>>,
    fft_thread_handle: Mutex<Option<JoinHandle<()>>>,
}

impl FftExecutor {
    pub fn new(
        tokio_runtime: Arc<Runtime>,
        num_bands: usize,
        tap_reader: Arc<TapReader>,
        fft_sender: Arc<sync::watch::Sender<FftData>>,
    ) -> Self {
        Self {
            tokio_runtime,
            num_bands,
            tap_reader,
            fft_sender,
            fft_thread_handle: Mutex::new(None),
        }
    }

    pub fn subscribe(&self) -> Result<impl Stream<Item = FftData>, AudioError> {
        let receiver = self.fft_sender.subscribe();
        Ok(WatchStream::new(receiver))
    }

    pub fn abort(&self) {
        let mut handle = self.fft_thread_handle.lock();
        if let Some(handle) = handle.take() {
            handle.abort();
        }
    }

    pub fn run(&self) {
        let mut handle = self.fft_thread_handle.lock();
        if let Some(handle) = handle.take() {
            handle.abort();
        }

        let cloned_num_bands = self.num_bands.clone();
        let cloned_tap_reader = self.tap_reader.clone();
        let cloned_sender = self.fft_sender.clone();
        *handle = Some(self.tokio_runtime.spawn_blocking(move || {
            run_custom_visualizer(
                cloned_tap_reader,
                4096,
                cloned_num_bands,
                false,
                cloned_sender,
            );
        }));
    }
}

impl Drop for FftExecutor {
    fn drop(&mut self) {
        let mut handle = self.fft_thread_handle.lock();
        if let Some(handle) = handle.take() {
            handle.abort();
        }
    }
}