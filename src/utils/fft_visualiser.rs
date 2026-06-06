use arrayvec::ArrayVec;
use realfft::{RealFftPlanner, RealToComplex};
use rodio_tap::{FrameReader, FrameReaderConfig, TapReader};
use rustfft::num_complex::Complex;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::Sender;

#[derive(Clone, Debug)]
pub struct FftChannelData {
    pub datas: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct FftData {
    pub sample_rate: u32,
    pub channel_datas: Vec<FftChannelData>,
}

#[derive(Debug, Clone)]
struct FrequencyBin {
    hz_lo: f32,
    hz_hi: f32,
}

fn generate_log_frequency_bands(
    sample_rate: u32,
    num_bands: usize,
    min_frequency: f32,
    max_frequency: f32,
) -> Vec<FrequencyBin> {
    let nyquist = sample_rate as f32 / 2.0;
    let effective_max = max_frequency.min(nyquist);
    let effective_min = min_frequency.max(1.0);
    if num_bands == 0 {
        return vec![];
    }
    let log_min = effective_min.ln();
    let log_max = effective_max.ln();
    let step = (log_max - log_min) / (num_bands as f32);
    let centers: Vec<f32> = (0..num_bands)
        .map(|i| (log_min + i as f32 * step).exp())
        .collect();
    let mut edges = vec![effective_min];
    for i in 0..num_bands - 1 {
        let mid = (centers[i] * centers[i + 1]).sqrt();
        edges.push(mid);
    }
    edges.push(effective_max);
    edges
        .windows(2)
        .map(|w| FrequencyBin {
            hz_lo: w[0],
            hz_hi: w[1],
        })
        .collect()
}

fn hz_to_bin(hz: f32, fft_len: usize, sample_rate: u32) -> usize {
    let idx = (hz * fft_len as f32 / sample_rate as f32).floor() as usize;
    idx.min(fft_len / 2)
}

fn hann_window(idx: usize, len: usize) -> f32 {
    if len <= 1 {
        1.0
    } else {
        let n = idx as f32;
        let denom = (len - 1) as f32;
        0.5 - 0.5 * (2.0 * std::f32::consts::PI * n / denom).cos()
    }
}

struct CustomSpectrumAnalyzer {
    fft_len: usize,
    bins: Vec<FrequencyBin>,
    channel_histories: Vec<Vec<f32>>,
    fft: Arc<dyn RealToComplex<f32>>,
    fft_input: Vec<f32>,
    fft_output: Vec<Complex<f32>>,
    normalize_by_fft_size: bool,
    current_sample_rate: u32,
}

impl CustomSpectrumAnalyzer {
    fn new(
        num_channels: usize,
        fft_len: usize,
        num_bands: usize,
        sample_rate: u32,
        min_frequency_hz: f32,
        max_frequency_hz: f32,
        normalize_by_fft_size: bool,
    ) -> Self {
        let bins = generate_log_frequency_bands(
            sample_rate,
            num_bands,
            min_frequency_hz,
            max_frequency_hz,
        );
        let mut fft_planner = RealFftPlanner::new();
        let fft = fft_planner.plan_fft_forward(fft_len);
        let fft_input = fft.make_input_vec();
        let fft_output = fft.make_output_vec();
        Self {
            fft_len,
            bins,
            channel_histories: vec![Vec::with_capacity(fft_len); num_channels],
            fft,
            fft_input,
            fft_output,
            normalize_by_fft_size,
            current_sample_rate: sample_rate,
        }
    }

    fn feed_samples<const C: usize>(
        &mut self,
        batch: &[ArrayVec<f32, C>],
        channels: usize,
    ) -> Option<Vec<Vec<f32>>> {
        if channels == 0 || batch.is_empty() {
            return None;
        }
        for hist in &mut self.channel_histories {
            if hist.capacity() < self.fft_len {
                hist.reserve(self.fft_len - hist.len());
            }
        }
        for frame in batch {
            for ch in 0..channels {
                if frame.len() > ch {
                    let sample = frame[ch];
                    let hist = &mut self.channel_histories[ch];
                    hist.push(sample);
                    if hist.len() > self.fft_len {
                        hist.remove(0);
                    }
                }
            }
        }
        if self
            .channel_histories
            .iter()
            .any(|h| h.len() < self.fft_len)
        {
            return None;
        }
        let mut all_channel_bins = Vec::with_capacity(channels);
        for ch in 0..channels {
            let hist = &self.channel_histories[ch];
            self.fft_input.fill(0.0);
            let start = self.fft_len - hist.len();
            for (i, &sample) in hist.iter().enumerate() {
                let idx = start + i;
                let windowed = sample * hann_window(idx, self.fft_len);
                self.fft_input[idx] = windowed;
            }
            let _ = self.fft.process(&mut self.fft_input, &mut self.fft_output);
            let mut magnitudes = vec![0.0; self.fft_output.len()];
            for (i, c) in self.fft_output.iter().enumerate() {
                let mut mag = (c.re * c.re + c.im * c.im).sqrt();
                if self.normalize_by_fft_size {
                    mag /= self.fft_len as f32;
                }
                magnitudes[i] = mag;
            }
            let bin_energies = self.aggregate_bins(&magnitudes);
            all_channel_bins.push(bin_energies);
        }
        Some(all_channel_bins)
    }

    fn aggregate_bins(&self, magnitudes: &[f32]) -> Vec<f32> {
        let sr = self.current_sample_rate;
        self.bins
            .iter()
            .map(|bin| {
                let lo_idx = hz_to_bin(bin.hz_lo, self.fft_len, sr);
                let hi_idx = hz_to_bin(bin.hz_hi, self.fft_len, sr);
                if lo_idx >= magnitudes.len() || hi_idx < lo_idx {
                    0.0
                } else {
                    let mut sum = 0.0;
                    let mut count = 0;
                    for idx in lo_idx..=hi_idx.min(magnitudes.len() - 1) {
                        sum += magnitudes[idx];
                        count += 1;
                    }
                    if count == 0 {
                        0.0
                    } else {
                        sum / count as f32
                    }
                }
            })
            .collect()
    }
}

impl FftData {
    pub fn empty() -> Self {
        FftData {
            sample_rate: 0,
            channel_datas: Vec::new(),
        }
    }
}

pub fn run_custom_visualizer<const C: usize>(
    tap_reader: Arc<TapReader<C>>,
    fft_len: usize,
    num_bands: usize,
    min_frequency_hz: f32,
    max_frequency_hz: f32,
    normalize_by_fft_size: bool,
    sender: Arc<Sender<FftData>>,
) -> ! {
    let reader_config = FrameReaderConfig {
        time_per_batch: Some(Duration::from_millis(30)),
        frames_per_batch: None,
        ..Default::default()
    };
    let mut reader =
        FrameReader::<C>::new_with_config(reader_config, move || Some(Arc::clone(&tap_reader)));
    let mut analyzer = None::<CustomSpectrumAnalyzer>;
    let mut last_sample_rate = 0;
    reader.run(move |batch, channels, sample_rate| {
        if channels != C {
            return;
        }
        if sample_rate != last_sample_rate {
            analyzer = Some(CustomSpectrumAnalyzer::new(
                C,
                fft_len,
                num_bands,
                sample_rate,
                min_frequency_hz,
                max_frequency_hz,
                normalize_by_fft_size,
            ));
            last_sample_rate = sample_rate;
        }
        if let Some(analyzer_ref) = &mut analyzer {
            if let Some(channel_bins) = analyzer_ref.feed_samples(batch, channels) {
                let channel_datas = channel_bins
                    .into_iter()
                    .map(|bins| FftChannelData { datas: bins })
                    .collect();
                let fft_data = FftData {
                    sample_rate,
                    channel_datas,
                };
                let _ = sender.send(fft_data);
            }
        }
    });
}
