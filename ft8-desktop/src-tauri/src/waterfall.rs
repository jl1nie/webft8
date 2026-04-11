use num_complex::Complex;
use rustfft::FftPlanner;
use serde::Serialize;
use std::f32::consts::PI;

const FFT_SIZE: usize = 1024;
const SAMPLE_RATE: f32 = 12000.0;
// After 4:1 decimation from 48kHz, waterfall gets 12kHz data.
// fftSize=1024 @ 12kHz → bin width = 11.72 Hz, time window = 85ms

/// Waterfall FFT result: power spectrum in dB
#[derive(Debug, Clone, Serialize)]
pub struct WaterfallRow {
    /// Power spectrum in dB (FFT_SIZE/2 bins, 0 to Nyquist)
    pub bins: Vec<f32>,
    /// Frequency resolution in Hz per bin
    pub bin_hz: f32,
}

/// Compute power spectrum for a chunk of audio samples.
/// Input: 1024 samples at 12 kHz.
/// Output: 512 bins covering 0 - 6000 Hz.
pub fn compute_waterfall(samples: &[f32]) -> WaterfallRow {
    let n = FFT_SIZE;
    assert!(samples.len() >= n, "Need at least {} samples", n);

    // Apply Hann window and pack into complex buffer
    let mut buffer: Vec<Complex<f32>> = samples[..n]
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w = 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos());
            Complex::new(s * w, 0.0)
        })
        .collect();

    // In-place FFT
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    fft.process(&mut buffer);

    // Power spectrum (first half only — real input is symmetric)
    let half = n / 2;
    let norm = 1.0 / (n as f32);
    let bins: Vec<f32> = buffer[..half]
        .iter()
        .map(|c| {
            let mag = (c.re * c.re + c.im * c.im).sqrt() * norm;
            // dB with floor at -120 dB
            20.0 * mag.max(1e-6).log10()
        })
        .collect();

    WaterfallRow {
        bins,
        bin_hz: SAMPLE_RATE / n as f32,
    }
}

/// Compute waterfall for a batch of overlapping frames.
/// Input: arbitrary length audio at 12 kHz.
/// Returns one WaterfallRow per hop (hop = FFT_SIZE/2 = 512 samples).
#[tauri::command]
pub fn waterfall_compute(samples: Vec<f32>) -> Vec<WaterfallRow> {
    let hop = FFT_SIZE / 2; // 50% overlap
    let mut rows = Vec::new();
    let mut offset = 0;
    while offset + FFT_SIZE <= samples.len() {
        rows.push(compute_waterfall(&samples[offset..]));
        offset += hop;
    }
    rows
}
