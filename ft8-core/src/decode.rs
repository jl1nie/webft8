/// High-level FT8 decode pipeline.
///
/// Chains: downsample → coarse_sync → fine_sync → LLR → BP decode
use rayon::prelude::*;

use crate::{
    downsample::{build_fft_cache, downsample},
    ldpc::{bp::bp_decode, osd::{osd_decode, osd_decode_deep}},
    llr::{compute_llr, compute_snr_db, symbol_spectra, sync_quality},
    params::BP_MAX_ITER,
    subtract::subtract_signal_weighted,
    sync::{coarse_sync, fine_sync_power_split, refine_candidate, SyncCandidate},
    wave_gen::message_to_tones,
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
    /// BP (all four variants) then OSD order-1 fallback when BP fails.
    BpAllOsd,
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
    /// Coefficient of variation of the three Costas-array powers (score_a/b/c).
    ///
    /// Near zero for a stable channel; elevated (> 0.3) when QSB or strong
    /// time-varying fading is present.  Used by `decode_frame_subtract` to
    /// apply partial subtraction gain when the amplitude estimate is unreliable.
    pub sync_cv: f32,
    /// WSJT-X compatible SNR estimate (dB).
    ///
    /// Computed from decoded tone power vs. opposite-tone noise power:
    /// `10 log10(xsig/xnoi − 1) − 27 dB`.  Floor is −24 dB (same as WSJT-X).
    pub snr_db: f32,
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
    decode_frame_inner(audio, freq_min, freq_max, sync_min, freq_hint, depth, max_cand, 2.5, &[])
}

// ────────────────────────────────────────────────────────────────────────────
// Per-candidate decode helper (used by both inner and sniper paths)

/// Decode a single sync candidate: downsample → refine → LLR → BP/OSD.
///
/// `fft_cache` — pre-computed 192 000-point forward FFT of the full audio
///   (from [`build_fft_cache`]), shared read-only across parallel calls.
/// `known`     — messages decoded in earlier subtract passes; prevents OSD
///   from running on frequencies that already have a result.
///
/// Returns `Some(DecodeResult)` on the first successful decode, `None` if the
/// candidate yields no valid message.
fn process_candidate(
    cand: &SyncCandidate,
    audio: &[i16],
    fft_cache: &[num_complex::Complex<f32>],
    depth: DecodeDepth,
    osd_score_min: f32,
    known: &[DecodeResult],
) -> Option<DecodeResult> {
    let (cd0, _) = downsample(audio, cand.freq_hz, Some(fft_cache));

    let refined = refine_candidate(&cd0, cand, 10);
    let i_start = ((refined.dt_sec + 0.5) * 200.0).round() as usize;
    let cs = symbol_spectra(&cd0, i_start);
    let nsync = sync_quality(&cs);
    if nsync <= 6 {
        return None;
    }

    // Costas-array power CV: near-zero for stable channel; > 0.3 implies QSB.
    let sync_cv = {
        let (sa, sb, sc) = fine_sync_power_split(&cd0, i_start);
        let mean = (sa + sb + sc) / 3.0;
        if mean > f32::EPSILON {
            let sq = (sa - mean).powi(2) + (sb - mean).powi(2) + (sc - mean).powi(2);
            sq.sqrt() / mean
        } else {
            0.0
        }
    };

    let llr_set = compute_llr(&cs);

    let llr_variants: &[(&[f32; 174], u8)] = match depth {
        DecodeDepth::Bp => &[(&llr_set.llra, 0)],
        DecodeDepth::BpAll | DecodeDepth::BpAllOsd => &[
            (&llr_set.llra, 0),
            (&llr_set.llrb, 1),
            (&llr_set.llrc, 2),
            (&llr_set.llrd, 3),
        ],
    };

    // ── BP decode ─────────────────────────────────────────────────────────────
    for &(llr, pass_id) in llr_variants {
        if let Some(bp) = bp_decode(llr, None, BP_MAX_ITER) {
            let itone = message_to_tones(&bp.message77);
            let snr_db = compute_snr_db(&*cs, &itone);
            return Some(DecodeResult {
                message77: bp.message77,
                freq_hz: cand.freq_hz,
                dt_sec: refined.dt_sec,
                hard_errors: bp.hard_errors,
                sync_score: refined.score,
                pass: pass_id,
                sync_cv,
                snr_db,
            });
        }
    }

    // ── OSD fallback ──────────────────────────────────────────────────────────
    if depth == DecodeDepth::BpAllOsd
        && nsync >= 12 && cand.score >= osd_score_min
    {
        // Avoid OSD if a known (previous-pass) result is already at this freq.
        let freq_dup = known.iter().any(|r| (r.freq_hz - cand.freq_hz).abs() < 20.0);
        if !freq_dup {
            let osd_depth: u8 = if nsync >= 18 { 3 } else { 2 };
            for llr_osd in [&llr_set.llra, &llr_set.llrb, &llr_set.llrc, &llr_set.llrd] {
                let osd_result = if osd_depth == 3 {
                    osd_decode_deep(llr_osd, 3)
                } else {
                    osd_decode(llr_osd)
                };
                if let Some(osd) = osd_result {
                    if osd.hard_errors >= 56 {
                        continue;
                    }
                    let itone = message_to_tones(&osd.message77);
                    let snr_db = compute_snr_db(&*cs, &itone);
                    return Some(DecodeResult {
                        message77: osd.message77,
                        freq_hz: cand.freq_hz,
                        dt_sec: refined.dt_sec,
                        hard_errors: osd.hard_errors,
                        sync_score: refined.score,
                        pass: 4,
                        sync_cv,
                        snr_db,
                    });
                }
            }
        }
    }

    None
}

// ────────────────────────────────────────────────────────────────────────────

/// Inner decode loop shared by [`decode_frame`] and [`decode_frame_subtract`].
///
/// `osd_score_min` — minimum coarse-sync score required for OSD fallback.
/// `known`         — messages already decoded in earlier passes (skipped).
fn decode_frame_inner(
    audio: &[i16],
    freq_min: f32,
    freq_max: f32,
    sync_min: f32,
    freq_hint: Option<f32>,
    depth: DecodeDepth,
    max_cand: usize,
    osd_score_min: f32,
    known: &[DecodeResult],
) -> Vec<DecodeResult> {
    let candidates = coarse_sync(audio, freq_min, freq_max, sync_min, freq_hint, max_cand);
    if candidates.is_empty() {
        return Vec::new();
    }

    // Pre-compute the 192 000-point forward FFT once; all per-candidate
    // calls share it read-only, enabling parallel processing.
    let fft_cache = build_fft_cache(audio);

    let raw: Vec<DecodeResult> = candidates
        .par_iter()
        .filter_map(|cand| process_candidate(cand, audio, &fft_cache, depth, osd_score_min, known))
        .collect();

    // Deduplicate: preserve first occurrence; drop messages already in `known`.
    let mut results: Vec<DecodeResult> = Vec::new();
    for r in raw {
        if !known.iter().any(|k| k.message77 == r.message77)
            && !results.iter().any(|x| x.message77 == r.message77)
        {
            results.push(r);
        }
    }
    results
}

// ────────────────────────────────────────────────────────────────────────────
// Multi-pass decode with signal subtraction

/// Decode a 15-second FT8 frame using successive signal subtraction.
///
/// Runs three decode passes with decreasing sync thresholds.  After each
/// pass every newly decoded signal is subtracted from the residual audio,
/// revealing weaker signals that were previously hidden.
///
/// | Pass | sync_min factor | OSD score min | Purpose |
/// |------|----------------|---------------|---------|
/// | 1    | 1.0×           | 2.5           | Strong signals (BP + OSD) |
/// | 2    | 0.75×          | 2.5           | Medium signals on residual |
/// | 3    | 0.5×           | 2.0           | Weak / spurious signals |
///
/// Pass 3 uses a lower OSD score threshold (`2.0` vs the normal `2.5`) to
/// also subtract signals that are marginal but have valid CRC — even if they
/// were questionable in the original audio, subtracting their reconstructed
/// waveform from the already-cleaned residual does more good than harm.
pub fn decode_frame_subtract(
    audio: &[i16],
    freq_min: f32,
    freq_max: f32,
    sync_min: f32,
    freq_hint: Option<f32>,
    depth: DecodeDepth,
    max_cand: usize,
) -> Vec<DecodeResult> {
    let mut residual = audio.to_vec();
    let mut all_results: Vec<DecodeResult> = Vec::new();

    // (sync_min_factor, osd_score_min)
    let passes: &[(f32, f32)] = &[(1.0, 2.5), (0.75, 2.5), (0.5, 2.0)];

    for &(factor, osd_score_min) in passes {
        let new = decode_frame_inner(
            &residual,
            freq_min, freq_max,
            sync_min * factor,
            freq_hint, depth, max_cand,
            osd_score_min,
            &all_results,
        );

        for r in &new {
            // QSB gate: if Costas-array power CV > 0.3 the channel is time-varying
            // and the amplitude estimate is less accurate — use half gain to avoid
            // over-subtraction artefacts that would corrupt later passes.
            let sub_gain = if r.sync_cv > 0.3 { 0.5 } else { 1.0 };
            subtract_signal_weighted(&mut residual, r, sub_gain);
        }
        all_results.extend(new);
    }

    all_results
}

// ────────────────────────────────────────────────────────────────────────────
// Convenience: sniper-mode decode (single target frequency, narrow band)

/// Sniper-mode decode: search only within ±250 Hz of `target_freq`.
///
/// Intended for use after a 500 Hz hardware BPF.  The search band is
/// narrowed to `target_freq ± 250 Hz` and `sync_min` is lowered to 0.8
/// because the BPF removes strong adjacent signals that would otherwise
/// raise the noise floor.
///
/// `sync_cv` (Costas-array power coefficient of variation) is computed for
/// each decoded result and can be used downstream as a channel-quality
/// indicator for the Phase 3 adaptive equaliser.
pub fn decode_sniper(
    audio: &[i16],
    target_freq: f32,
    depth: DecodeDepth,
    max_cand: usize,
) -> Vec<DecodeResult> {
    let freq_min = (target_freq - 250.0).max(100.0);
    let freq_max = (target_freq + 250.0).min(5900.0);
    let sync_min = 0.8;

    let candidates = coarse_sync(audio, freq_min, freq_max, sync_min, Some(target_freq), max_cand);
    if candidates.is_empty() {
        return Vec::new();
    }

    let fft_cache = build_fft_cache(audio);

    let raw: Vec<DecodeResult> = candidates
        .par_iter()
        .filter_map(|cand| process_candidate(cand, audio, &fft_cache, depth, 2.5, &[]))
        .collect();

    let mut results: Vec<DecodeResult> = Vec::new();
    for r in raw {
        if !results.iter().any(|x| x.message77 == r.message77) {
            results.push(r);
        }
    }
    results
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
