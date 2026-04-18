//! WSPR receiver path: audio samples → 162 per-symbol data-bit LLRs.
//!
//! ## Geometry
//!
//! The only piece of luck the protocol hands us: at 12 kHz sample rate
//! and `NSPS = 8192`, a single-symbol FFT has bin width `12000/8192 =
//! 1.4648 Hz`, exactly one WSPR tone spacing. So a 256-sample FFT at
//! 375 Hz, or an 8192-sample FFT at 12 kHz, lands each tone on its own
//! bin with no leakage between tones. We take the 12 kHz version
//! directly — no downsampling step, no polyphase filter — one FFT per
//! symbol gives the four tone powers we need.
//!
//! ## What this module does
//!
//! Given already-aligned audio (caller knows the start sample and base
//! frequency), emit 162 LLRs — one per channel symbol, in **coded-bit
//! order** (i.e. still interleaved, matching the order the convolutional
//! encoder produced). The caller runs [`crate::deinterleave`] on the
//! LLRs and feeds them to the Fano decoder.
//!
//! ## What this module does *not* do
//!
//! No coarse frequency search, no time-offset refinement. The caller
//! must supply the approximate base frequency (the "tone 0" bin) and
//! the nominal audio start index. A follow-up module will wrap this
//! with a peak-search over the sync-vector correlation metric.

use mfsk_core::ModulationParams;
use num_complex::Complex;
use rustfft::FftPlanner;

use crate::{Wspr, WSPR_SYNC_VECTOR};

/// Demodulate 162 channel symbols from aligned audio and produce per-
/// symbol data-bit LLRs.
///
/// * `audio` — mono `f32` samples at `sample_rate` Hz. Must contain at
///   least `start_sample + 162 * NSPS` samples (where NSPS scales from
///   the trait constant to the actual sample rate).
/// * `start_sample` — index of the first sample of symbol 0.
/// * `base_freq_hz` — frequency of tone 0. Tones 1/2/3 are assumed at
///   `base_freq + n * 1.4648 Hz`.
///
/// Returns LLRs in the convention: **positive → data bit 0 more likely**.
pub fn demodulate_aligned(
    audio: &[f32],
    sample_rate: u32,
    start_sample: usize,
    base_freq_hz: f32,
) -> [f32; 162] {
    let nsps = (sample_rate as f32 * <Wspr as ModulationParams>::SYMBOL_DT).round() as usize;
    let df = sample_rate as f32 / nsps as f32; // = TONE_SPACING_HZ by construction
    // Round the base frequency to the nearest bin — caller is expected to
    // land within ±0.5 bin of the true frequency.
    let base_bin = (base_freq_hz / df).round() as usize;
    let tone_bins = [
        base_bin,
        base_bin + 1,
        base_bin + 2,
        base_bin + 3,
    ];

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(nsps);
    let mut scratch = vec![Complex::new(0.0f32, 0.0); fft.get_inplace_scratch_len()];
    let mut buf: Vec<Complex<f32>> = vec![Complex::new(0.0f32, 0.0); nsps];

    // Accumulate per-symbol data-bit magnitudes then convert to LLR with
    // a single global noise estimate derived from off-tone bins.
    let mut m_even = [0.0f32; 162]; // magnitude if data=0 (tone index = sync)
    let mut m_odd = [0.0f32; 162]; // magnitude if data=1 (tone index = sync + 2)
    let mut noise_acc = 0.0f32;
    let mut noise_count = 0u32;

    for i in 0..162 {
        let sym_start = start_sample + i * nsps;
        if sym_start + nsps > audio.len() {
            // Past the end — leave LLR at 0 (uninformative) for remaining symbols.
            break;
        }
        // Copy real audio into complex buffer.
        for (slot, &s) in buf.iter_mut().zip(&audio[sym_start..sym_start + nsps]) {
            *slot = Complex::new(s, 0.0);
        }
        fft.process_with_scratch(&mut buf, &mut scratch);

        // Magnitude of each tone bin.
        let mags: [f32; 4] = [
            buf[tone_bins[0]].norm(),
            buf[tone_bins[1]].norm(),
            buf[tone_bins[2]].norm(),
            buf[tone_bins[3]].norm(),
        ];

        // Accumulate noise from bins just above the tones (base+4, base+5)
        // to estimate σ for LLR scaling. Assumes the receiver has cropped
        // the passband tightly enough that nearby bins are WGN.
        for k in 4..8 {
            let bin = base_bin + k;
            if bin < buf.len() / 2 {
                noise_acc += buf[bin].norm_sqr();
                noise_count += 1;
            }
        }

        let sync = WSPR_SYNC_VECTOR[i];
        if sync == 0 {
            // data 0 → tone 0, data 1 → tone 2
            m_even[i] = mags[0];
            m_odd[i] = mags[2];
        } else {
            // data 0 → tone 1, data 1 → tone 3
            m_even[i] = mags[1];
            m_odd[i] = mags[3];
        }
    }

    // Noise variance estimate (|bin|² has χ²_2 distribution → mean ≈ σ²).
    // Floor the estimate against the per-symbol power so high-SNR (or
    // noise-free synthetic) inputs still produce finite LLRs.
    let mean_sig_power = m_even
        .iter()
        .chain(m_odd.iter())
        .map(|&m| m * m)
        .sum::<f32>()
        / (2.0 * 162.0);
    let sigma2 = if noise_count > 0 {
        (noise_acc / noise_count as f32).max(mean_sig_power * 1e-4)
    } else {
        mean_sig_power.max(1.0)
    };

    // Noncoherent 4-FSK LLR: ≈ (|m_even|² - |m_odd|²) / σ². Clamped to
    // ±20 so downstream integer-metric Fano stays in range.
    let mut llrs = [0f32; 162];
    for i in 0..162 {
        let raw = (m_even[i] * m_even[i] - m_odd[i] * m_odd[i]) / sigma2;
        llrs[i] = raw.clamp(-20.0, 20.0);
    }
    llrs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::synthesize_audio;

    #[test]
    fn recovers_llr_sign_noise_free() {
        // Symbols with alternating data bits, sync forced to zero for
        // simplicity (fake sync — real sync comes from WSPR_SYNC_VECTOR).
        let mut symbols = [0u8; 162];
        for i in 0..162 {
            let data_bit = (i & 1) as u8;
            let sync = WSPR_SYNC_VECTOR[i];
            symbols[i] = 2 * data_bit + sync;
        }
        let audio = synthesize_audio(&symbols, 12_000, 1500.0, 0.3);
        let llrs = demodulate_aligned(&audio, 12_000, 0, 1500.0);

        // Each LLR's sign should match the data bit: bit=0 → positive.
        for i in 0..162 {
            let expect_positive = (i & 1) == 0;
            if expect_positive {
                assert!(llrs[i] > 0.0, "symbol {} LLR should be > 0, got {}", i, llrs[i]);
            } else {
                assert!(llrs[i] < 0.0, "symbol {} LLR should be < 0, got {}", i, llrs[i]);
            }
        }
    }
}
