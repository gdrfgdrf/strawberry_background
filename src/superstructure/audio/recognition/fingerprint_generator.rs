use std::io::Write;
use std::sync::OnceLock;

use aes::Aes128;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockEncrypt, KeyInit};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use flate2::{Compression, write::ZlibEncoder};
use realfft::RealFftPlanner;

pub const SAMPLE_RATE: usize = 8_000;
const WINDOW_SIZE: usize = 2_048;
const HOP_SIZE: usize = 160;
const BIN_COUNT: usize = WINDOW_SIZE / 2 + 1;
const BIN_HZ: f64 = SAMPLE_RATE as f64 / WINDOW_SIZE as f64;
const LOW_BIN: usize = (100.0 / BIN_HZ) as usize;
const HIGH_BIN: usize = (4_000.0 / BIN_HZ) as usize;
const BAND_BINS: usize = HIGH_BIN - LOW_BIN;
const MIN_FRAMES: usize = 10;
const MIN_MAGNITUDE: f32 = f32::from_bits(0x3596_37bd);

const VERSION: &[u8] = b"hyai_1.2.0_client_1.0.0";
const AES_KEY: &[u8; 16] = b"4B97221F27F02907";

const HAMMING_TABLE: &[u8; WINDOW_SIZE * 4] = include_bytes!("tables/hamming_2048.f32le");

static HAMMING_WINDOW: OnceLock<Vec<f32>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct Fingerprint {
    pub encoded: String,
}

#[derive(Debug, Clone, Copy)]
struct Peak {
    frequency_bin: usize,
    time_frame: usize,
    amplitude: f32,
}

#[derive(Debug, Clone, Copy)]
struct ExtractConfig {
    final_frequency_radius: usize,
    final_time_radius: usize,
    postprocess_enabled: bool,
    postprocess_mode: u8,
    average_frequency_radius: usize,
    average_time_radius: usize,
    average_threshold: f64,
}

const DEFAULT_EXTRACT_CONFIG: ExtractConfig = ExtractConfig {
    final_frequency_radius: 30,
    final_time_radius: 8,
    postprocess_enabled: true,
    postprocess_mode: 1,
    average_frequency_radius: 10,
    average_time_radius: 5,
    average_threshold: 1.0,
};

#[tracing::instrument(skip(samples))]
pub fn generate(samples: &[f32]) -> Result<Fingerprint, String> {
    let generated = generate_binary(samples)?;
    Ok(Fingerprint {
        encoded: BASE64_STANDARD.encode(generated.encrypted),
    })
}

pub fn extract_query_fp(samples: &[f32]) -> Result<Vec<u8>, String> {
    Ok(generate_binary(samples)?.encrypted)
}

#[derive(Debug)]
struct BinaryFingerprint {
    encrypted: Vec<u8>,
}

fn generate_binary(samples: &[f32]) -> Result<BinaryFingerprint, String> {
    if samples.len() < WINDOW_SIZE {
        return Err(format!(
            "audio is too short: need at least {:.3} seconds at 8 kHz",
            WINDOW_SIZE as f64 / SAMPLE_RATE as f64
        ));
    }

    let duration_seconds = samples.len() as f32 / SAMPLE_RATE as f32;
    let power_frames = stft_power(samples)?;
    let feature_matrix = build_feature_matrix(&power_frames);
    let peaks = extract_peaks(&feature_matrix, DEFAULT_EXTRACT_CONFIG);
    let raw = build_raw_fingerprint(duration_seconds, &peaks)?;
    let encrypted = encrypt_raw_fingerprint(&raw)?;

    Ok(BinaryFingerprint { encrypted })
}

fn hamming_window() -> &'static [f32] {
    HAMMING_WINDOW.get_or_init(|| {
        HAMMING_TABLE
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect()
    })
}

fn stft_power(samples: &[f32]) -> Result<Vec<Vec<f32>>, String> {
    if samples.len() < WINDOW_SIZE {
        return Ok(Vec::new());
    }

    let window = hamming_window();
    let frame_count = (samples.len() - WINDOW_SIZE) / HOP_SIZE + 1;

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(WINDOW_SIZE);
    let mut scratch = fft.make_scratch_vec();
    let mut output = fft.make_output_vec(); // length = BIN_COUNT
    let mut input = vec![0.0f32; WINDOW_SIZE];

    let mut frames = Vec::with_capacity(frame_count);

    for frame in 0..frame_count {
        let start = frame * HOP_SIZE;
        for i in 0..WINDOW_SIZE {
            input[i] = samples[start + i] * window[i];
        }

        fft.process_with_scratch(&mut input, &mut output, &mut scratch)
            .map_err(|e| e.to_string())?;

        let mut power = Vec::with_capacity(BIN_COUNT);
        for value in output.iter() {
            power.push(value.re * value.re + value.im * value.im);
        }
        frames.push(power);
    }

    Ok(frames)
}

fn wasm_logf(mut value: f32) -> f32 {
    const TWO_POW_25: f32 = f32::from_bits(0x4c00_0000);
    const LN2_HI: f32 = f32::from_bits(0x3f31_7180);
    const LN2_LO: f32 = f32::from_bits(0x3717_f7d1);
    const LG1: f32 = f32::from_bits(0x3f2a_aaaa);
    const LG2: f32 = f32::from_bits(0x3ecc_ce13);
    const LG3: f32 = f32::from_bits(0x3e91_e9ee);
    const LG4: f32 = f32::from_bits(0x3e78_9e26);
    const NORMALIZED_MANTISSA_BASE: u32 = 0x3f35_04f3;
    const ROUNDING_BIAS: u32 = 0x004a_fb0d;

    let mut bits = value.to_bits();
    let mut exponent_adjustment: i32;

    if bits < 0x0080_0000 || (bits as i32) < 0 {
        if bits & 0x7fff_ffff == 0 {
            return -1.0 / (value * value);
        }
        if (bits as i32) < 0 {
            return (value - value) / 0.0;
        }

        value *= TWO_POW_25;
        bits = value.to_bits();
        exponent_adjustment = -152;
    } else {
        if bits > 0x7f7f_ffff {
            return value;
        }
        if bits == 0x3f80_0000 {
            return 0.0;
        }
        exponent_adjustment = -127;
    }

    bits = bits.wrapping_add(ROUNDING_BIAS);
    exponent_adjustment += (bits >> 23) as i32;
    bits = (bits & 0x007f_ffff).wrapping_add(NORMALIZED_MANTISSA_BASE);

    let f = f32::from_bits(bits) - 1.0;
    let s = f / (2.0 + f);
    let half_f_squared = (0.5 * f) * f;
    let z = s * s;
    let w = z * z;
    let t1 = w * (LG2 + w * LG4);
    let t2 = z * (LG1 + w * LG3);
    let remainder = t2 + t1;
    let exponent = exponent_adjustment as f32;

    let mut result = s * (half_f_squared + remainder);
    result += exponent * LN2_LO;
    result -= half_f_squared;
    result += f;
    result += exponent * LN2_HI;
    result
}

fn build_feature_matrix(power_frames: &[Vec<f32>]) -> Vec<Vec<f32>> {
    let frame_count = power_frames.len();
    if frame_count == 0 {
        return Vec::new();
    }

    let mut matrix = vec![vec![0.0_f32; frame_count]; BAND_BINS];
    let mut sum = 0.0_f64;
    let mut count = 0_usize;

    for (time, power) in power_frames.iter().enumerate() {
        for frequency in 0..BAND_BINS {
            let magnitude = power[LOW_BIN + frequency].sqrt();
            let clamped = if magnitude >= MIN_MAGNITUDE {
                magnitude
            } else {
                MIN_MAGNITUDE
            };
            let value = wasm_logf(clamped);
            matrix[frequency][time] = value;
            sum += value as f64;
            count += 1;
        }
    }

    let mean = (sum / count as f64) as f32;
    for row in &mut matrix {
        for value in row {
            *value -= mean;
        }
    }

    matrix
}

fn has_greater_in_neighborhood(
    matrix: &[Vec<f32>],
    frequency: usize,
    time: usize,
    frequency_radius: usize,
    time_radius: usize,
) -> bool {
    let band_bins = matrix.len();
    let frame_count = matrix.first().map_or(0, Vec::len);
    let center = matrix[frequency][time];
    let frequency_start = frequency.saturating_sub(frequency_radius);
    let frequency_end = (frequency + frequency_radius + 1).min(band_bins);
    let time_start = time.saturating_sub(time_radius);
    let time_end = (time + time_radius + 1).min(frame_count);

    matrix[frequency_start..frequency_end].iter().any(|row| {
        row[time_start..time_end]
            .iter()
            .any(|value| *value > center)
    })
}

fn filter_by_local_average(
    peaks: Vec<Peak>,
    matrix: &[Vec<f32>],
    config: ExtractConfig,
) -> Vec<Peak> {
    let band_bins = matrix.len();
    let frame_count = matrix.first().map_or(0, Vec::len);
    let mut kept = Vec::with_capacity(peaks.len());

    for peak in peaks {
        let amplitude = matrix[peak.frequency_bin][peak.time_frame];
        if amplitude <= 0.0 {
            continue;
        }

        let frequency_start = peak
            .frequency_bin
            .saturating_sub(config.average_frequency_radius);
        let frequency_end =
            (peak.frequency_bin + config.average_frequency_radius + 1).min(band_bins);
        let time_start = peak.time_frame.saturating_sub(config.average_time_radius);
        let time_end = (peak.time_frame + config.average_time_radius + 1).min(frame_count);

        if frequency_start >= frequency_end || time_start >= time_end {
            continue;
        }

        let mut sum = 0.0_f64;
        for row in &matrix[frequency_start..frequency_end] {
            for value in &row[time_start..time_end] {
                sum += *value as f64;
            }
        }

        let cell_count = (frequency_end - frequency_start) * (time_end - time_start);
        let average = sum / cell_count as f64;
        if average > 2.0 || amplitude as f64 - average > config.average_threshold {
            kept.push(peak);
        }
    }

    kept
}

fn filter_by_f227_shape(peaks: Vec<Peak>, matrix: &[Vec<f32>]) -> Vec<Peak> {
    let band_bins = matrix.len();
    let frame_count = matrix.first().map_or(0, Vec::len);
    let mut kept = Vec::with_capacity(peaks.len());

    for peak in peaks {
        let frequency_start = peak.frequency_bin.saturating_sub(1);
        let frequency_end = (peak.frequency_bin + 2).min(band_bins);
        let time_start = peak.time_frame.saturating_sub(1);
        let time_end = (peak.time_frame + 2).min(frame_count);
        let mut reject = false;

        'outer: for frequency in frequency_start..frequency_end {
            for time in time_start..time_end {
                let center = matrix[frequency][time];
                let near_frequency_start = frequency.saturating_sub(1);
                let near_frequency_end = (frequency + 2).min(band_bins);
                let near_time_start = time.saturating_sub(1);
                let near_time_end = (time + 2).min(frame_count);

                let mut has_lower = false;
                for row in &matrix[near_frequency_start..near_frequency_end] {
                    if row[near_time_start..near_time_end]
                        .iter()
                        .any(|value| *value < center)
                    {
                        has_lower = true;
                        break;
                    }
                }

                if !has_lower {
                    reject = true;
                    break 'outer;
                }
            }
        }

        if !reject {
            kept.push(peak);
        }
    }

    kept
}

fn extract_peaks(matrix: &[Vec<f32>], config: ExtractConfig) -> Vec<Peak> {
    let band_bins = matrix.len();
    let frame_count = matrix.first().map_or(0, Vec::len);
    if frame_count < MIN_FRAMES {
        return Vec::new();
    }

    let mut peaks = Vec::new();
    for frequency in 0..band_bins {
        for time in 0..frame_count {
            if !has_greater_in_neighborhood(matrix, frequency, time, 1, 1) {
                peaks.push(Peak {
                    frequency_bin: frequency,
                    time_frame: time,
                    amplitude: matrix[frequency][time],
                });
            }
        }
    }

    if config.postprocess_enabled {
        peaks = match config.postprocess_mode {
            1 => filter_by_local_average(peaks, matrix, config),
            2 => filter_by_f227_shape(peaks, matrix),
            3 => filter_by_f227_shape(filter_by_local_average(peaks, matrix, config), matrix),
            _ => peaks,
        };
    }

    peaks.retain(|peak| {
        !has_greater_in_neighborhood(
            matrix,
            peak.frequency_bin,
            peak.time_frame,
            config.final_frequency_radius,
            config.final_time_radius,
        )
    });

    for peak in &mut peaks {
        peak.frequency_bin += LOW_BIN;
    }
    peaks.sort_by_key(|peak| (peak.time_frame, peak.frequency_bin));
    peaks
}

fn build_raw_fingerprint(duration_seconds: f32, peaks: &[Peak]) -> Result<Vec<u8>, String> {
    let mut raw = Vec::with_capacity(79 + peaks.len() * 12);
    raw.extend_from_slice(&(VERSION.len() as u32).to_le_bytes());
    raw.extend_from_slice(VERSION);
    raw.extend_from_slice(&[0_u8; 8]);
    raw.extend_from_slice(&duration_seconds.to_le_bytes());
    raw.extend_from_slice(b"FPVER");
    raw.extend_from_slice(&(VERSION.len() as u32).to_le_bytes());
    raw.extend_from_slice(VERSION);
    raw.extend_from_slice(b"Peak");
    raw.extend_from_slice(&(peaks.len() as u32).to_le_bytes());

    for peak in peaks {
        raw.extend_from_slice(&(peak.frequency_bin as u32).to_le_bytes());
        raw.extend_from_slice(&(peak.time_frame as u32).to_le_bytes());
        raw.extend_from_slice(&peak.amplitude.to_le_bytes());
    }

    Ok(raw)
}

fn encrypt_raw_fingerprint(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(0));
    encoder.write_all(raw).map_err(|e| e.to_string())?;
    let mut encrypted = encoder.finish().map_err(|e| e.to_string())?;

    let padding_length = 16 - encrypted.len() % 16;
    encrypted.resize(encrypted.len() + padding_length, padding_length as u8);

    let cipher = Aes128::new_from_slice(AES_KEY).map_err(|e| e.to_string())?;
    for chunk in encrypted.chunks_exact_mut(16) {
        let block = GenericArray::from_mut_slice(chunk);
        cipher.encrypt_block(block);
    }

    Ok(encrypted)
}

#[cfg(test)]
mod tests {
    use crate::superstructure::audio::recognition::fingerprint_generator::generate;
    use std::error::Error;
}
