/// High-level FT8 decode pipeline.
///
/// Chains: downsample → coarse_sync → fine_sync → LLR → BP decode
use crate::{
    downsample::downsample,
    ldpc::bp::bp_decode,
    llr::{compute_llr, symbol_spectra, sync_quality},
    params::BP_MAX_ITER,
    sync::{coarse_sync, refine_candidate},
};

// ────────────────────────────────────────────────────────────────────────────
// Public types

/// Decoding depth: which LLR sets and passes to attempt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DecodeDepth {
    /// Belief-propagation only, using nsym=1 metrics (fast).
    Bp,
    /// BP with all four metric variants (a, b, c, d).
    BpAll,
}

/// One successfully decoded FT8 message.
#[derive(Debug, Clone)]
pub struct DecodeResult {
    /// Decoded message: 77 bits packed as bytes (LSB first within each byte).
    pub message77: [u8; 77],
    /// Carrier frequency (Hz)
    pub freq_hz: f32,
    /// Time offset from the nominal 0.5 s start (seconds)
    pub dt_sec: f32,
    /// Number of hard-decision errors in the final codeword
    pub hard_errors: u32,
    /// Sync quality score from fine sync
    pub sync_score: f32,
    /// Which LLR variant decoded successfully (0=llra, 1=llrb, 2=llrc, 3=llrd)
    pub pass: u8,
}

// ────────────────────────────────────────────────────────────────────────────
// Main decode entry point

/// Decode one 15-second FT8 audio frame.
///
/// # Arguments
/// * `audio`      — 16-bit PCM samples at 12 000 Hz, length ≤ 180 000
/// * `freq_min`   — lower edge of search band (Hz)
/// * `freq_max`   — upper edge of search band (Hz)
/// * `sync_min`   — minimum coarse-sync score (typical: 1.0–2.0)
/// * `freq_hint`  — optional preferred frequency; matching candidates are tried first
/// * `depth`      — decoding depth
/// * `max_cand`   — maximum number of sync candidates to evaluate
///
/// Returns all successfully decoded messages (deduplicated by `message77`).
pub fn decode_frame(
    audio: &[i16],
    freq_min: f32,
    freq_max: f32,
    sync_min: f32,
    freq_hint: Option<f32>,
    depth: DecodeDepth,
    max_cand: usize,
) -> Vec<DecodeResult> {
    // ── Coarse sync ─────────────────────────────────────────────────────────
    let candidates = coarse_sync(audio, freq_min, freq_max, sync_min, freq_hint, max_cand);

    // Cache the large forward FFT (computed once per frame, reused per candidate)
    let mut fft_cache: Option<Vec<_>> = None;

    let mut results: Vec<DecodeResult> = Vec::new();

    for cand in &candidates {
        // ── Downsample to 200 Hz centred on candidate frequency ─────────────
        let (cd0, new_cache) = downsample(audio, cand.freq_hz, fft_cache.as_deref());
        fft_cache = Some(new_cache);

        // ── Fine sync (refine time offset at downsampled rate) ───────────────
        let refined = refine_candidate(&cd0, cand, 10);

        let i_start = (refined.dt_sec * 200.0).round() as usize;

        // ── Symbol spectra (79 × 8 complex bins) ────────────────────────────
        let cs = symbol_spectra(&cd0, i_start);

        // Bail out on poor sync quality
        let nsync = sync_quality(&cs);
        if nsync <= 6 {
            continue;
        }

        // ── LLR computation ──────────────────────────────────────────────────
        let llr_set = compute_llr(&cs);

        // ── BP decode — try multiple LLR variants ───────────────────────────
        let llr_variants: &[(&[f32; 174], u8)] = match depth {
            DecodeDepth::Bp => &[(&llr_set.llra, 0)],
            DecodeDepth::BpAll => &[
                (&llr_set.llra, 0),
                (&llr_set.llrb, 1),
                (&llr_set.llrc, 2),
                (&llr_set.llrd, 3),
            ],
        };

        for &(llr, pass_id) in llr_variants {
            if let Some(bp) = bp_decode(llr, None, BP_MAX_ITER) {
                let result = DecodeResult {
                    message77: bp.message77,
                    freq_hz: cand.freq_hz,
                    dt_sec: refined.dt_sec,
                    hard_errors: bp.hard_errors,
                    sync_score: refined.score,
                    pass: pass_id,
                };
                // Deduplicate by message77
                if !results.iter().any(|r| r.message77 == result.message77) {
                    results.push(result);
                }
                break; // first successful pass wins for this candidate
            }
        }
    }

    results
}

// ────────────────────────────────────────────────────────────────────────────
// Convenience: sniper-mode decode (single target frequency, narrow band)

/// Sniper-mode decode: search only within ±250 Hz of `target_freq`.
///
/// Lower `sync_min` is used because the 500 Hz filter removes strong
/// adjacent signals and reduces the noise floor.
pub fn decode_sniper(
    audio: &[i16],
    target_freq: f32,
    depth: DecodeDepth,
    max_cand: usize,
) -> Vec<DecodeResult> {
    let freq_min = (target_freq - 250.0).max(100.0);
    let freq_max = (target_freq + 250.0).min(5900.0);
    let sync_min = 0.8; // lower threshold: strong neighbours are gone

    decode_frame(audio, freq_min, freq_max, sync_min, Some(target_freq), depth, max_cand)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Silence produces no decoded messages and does not panic.
    #[test]
    fn silence_no_decode() {
        let audio = vec![0i16; 15 * 12_000];
        let results = decode_frame(&audio, 200.0, 2800.0, 1.0, None, DecodeDepth::Bp, 10);
        assert!(results.is_empty(), "silence should decode nothing");
    }

    /// Sniper mode on silence also produces no decoded messages.
    #[test]
    fn sniper_silence_no_decode() {
        let audio = vec![0i16; 15 * 12_000];
        let results = decode_sniper(&audio, 1000.0, DecodeDepth::Bp, 10);
        assert!(results.is_empty());
    }
}
