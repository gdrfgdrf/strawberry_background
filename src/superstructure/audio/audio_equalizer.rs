use parking_lot::{Mutex, RwLock};
use rodio::{ChannelCount, SampleRate, Source};
use simple_eq::Equalizer;
use std::sync::Arc;
use std::time::Duration;

const FREQUENCIES: [f32; 32] = [
    16.0, 20.0, 25.0, 32.0, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 320.0,
    400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3200.0, 4000.0, 5000.0,
    6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
];

pub struct EqualizerSource<S> {
    source: S,
    equalizer: Equalizer,
}

pub struct ArcEqualizerSource<S> {
    source: Arc<RwLock<EqualizerSource<S>>>,
}

impl<S> EqualizerSource<S> {
    pub fn new(source: S, sample_rate: u32) -> Self {
        let mut equalizer = Equalizer::new(sample_rate as f32);

        for (i, frequency) in FREQUENCIES.iter().enumerate() {
            equalizer.set_frequency(i, frequency.clone());
        }

        EqualizerSource { source, equalizer }
    }

    pub fn set_gain(&mut self, index: usize, gain: f32) {
        self.equalizer.set_gain(index, gain);
    }
}

impl<S> ArcEqualizerSource<S> {
    pub fn new(source: S, sample_rate: u32) -> Self {
        let equalizer_source = EqualizerSource::new(source, sample_rate);
        Self {
            source: Arc::new(RwLock::new(equalizer_source)),
        }
    }

    pub fn set_gain(&self, index: usize, gain: f32) {
        let mut source = self.source.write();
        source.set_gain(index, gain)
    }
}

impl<S> Source for EqualizerSource<S>
where
    S: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.source.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.source.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
}

impl<S> Source for ArcEqualizerSource<S>
where
    S: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        let source = self.source.read();
        source.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        let source = self.source.read();
        source.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        let source = self.source.read();
        source.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        let source = self.source.read();
        source.total_duration()
    }
}

impl<S> Iterator for EqualizerSource<S>
where
    S: Source,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(sample) = self.source.next() {
            Some(self.equalizer.process(sample))
        } else {
            None
        }
    }
}

impl<S> Iterator for ArcEqualizerSource<S>
where
    S: Source,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let mut source = self.source.write();
        let sample = source.source.next();
        if sample.is_none() {
            return None;
        }
        let sample = sample.unwrap();
        Some(source.equalizer.process(sample))
    }
}

impl<S> Clone for ArcEqualizerSource<S> {
    fn clone(&self) -> Self {
        ArcEqualizerSource {
            source: self.source.clone()
        }
    }
}

unsafe impl<S> Send for ArcEqualizerSource<S> {}
