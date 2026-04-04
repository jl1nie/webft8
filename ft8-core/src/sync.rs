/// FT8 synchronisation (coarse + fine).
/// Ported from WSJT-X sync8.f90 (coarse) and sync8d.f90 (fine).
use num_complex::Complex;
use rustfft::FftPlanner;
use std::f32::consts::PI;

use crate::params::{COSTAS, DF, NHSYM, NH1, NFFT1, NSPS, NSTEP, TSTEP};

// ────────────────────────────────────────────────────────────────────────────
// Constants

/// Max ±2.5 s lag in 1/4-symbol steps (62.5 → integer 62)
const JZ: i32 = 62;
/// Steps per symbol (nssy = NSPS/NSTEP = 4)
const NSSY: usize = NSPS / NSTEP; // 4
/// Frequency oversampling factor (nfos = NFFT1/NSPS = 2)
const NFOS: usize = NFFT1 / NSPS; // 2
/// Time index of the start of the first FT8 symbol (0.5 s into the 15-s window)
/// jstrt = int(0.5 / tstep)
const JSTRT: i32 = (0.5 / TSTEP) as i32; // 12

// ────────────────────────────────────────────────────────────────────────────
// Public types

/// One synchronisation candidate (matches WSJT-X `candidate(3,maxcand)` layout).
#[derive(Debug, Clone)]
pub struct SyncCandidate {
    /// Carrier frequency (Hz)
    pub freq_hz: f32,
    /// Time offset relative to the nominal 0.5 s start (seconds)
    pub dt_sec: f32,
    /// Normalised sync score (larger = better)
    pub score: f32,
}

// ────────────────────────────────────────────────────────────────────────────
// Coarse sync (sync8)

/// Compute per-symbol power spectra from raw audio.
///
/// Returns an array `s[freq_bin][time_step]` of size `[NH1][NHSYM]`.
/// Equivalent to the WSJT-X spectrum computation in sync8.f90 lines 28-43.
pub fn compute_spectra(audio: &[i16]) -> Box<[[f32; NHSYM]]> {
    let fac = 1.0f32 / 300.0;
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(NFFT1);

    // Allocate [NH1][NHSYM] on the heap
    let mut s: Box<[[f32; NHSYM]]> = vec![[0.0f32; NHSYM]; NH1].into_boxed_slice();

    let mut buf = vec![Complex::new(0.0f32, 0.0); NFFT1];
    for j in 0..NHSYM {
        let ia = j * NSTEP;
        let _ib = ia + NSPS;
        // Fill real part from audio, imaginary = 0; scale by fac
        for (k, c) in buf.iter_mut().enumerate() {
            if k < NSPS {
                let sample = if ia + k < audio.len() {
                    audio[ia + k] as f32 * fac
                } else {
                    0.0
                };
                *c = Complex::new(sample, 0.0);
            } else {
                *c = Complex::new(0.0, 0.0);
            }
        }
        fft.process(&mut buf);
        // Store magnitude squared for bins 0..NH1
        for i in 0..NH1 {
            s[i][j] = buf[i].norm_sqr();
        }
    }
    s
}

/// Coarse sync: search for FT8 frame candidates in `audio`.
///
/// * `freq_min`, `freq_max` — frequency search range (Hz)
/// * `sync_min` — minimum normalised score threshold
/// * `freq_hint` — optional: preferred frequency (Hz); matching candidates are placed first
/// * `max_cand` — maximum candidates to return
///
/// Returns candidates sorted by score (best first), with `freq_hint` matches leading.
pub fn coarse_sync(
    audio: &[i16],
    freq_min: f32,
    freq_max: f32,
    sync_min: f32,
    freq_hint: Option<f32>,
    max_cand: usize,
) -> Vec<SyncCandidate> {
    let s = compute_spectra(audio);

    let ia = (freq_min / DF).round() as usize;
    let ib = ((freq_max / DF).round() as usize).min(NH1 - 13); // leave room for 7 tones

    // ── build 2D sync map ─────────────────────────────────────────────────
    // sync2d[freq_bin - ia][lag_offset + JZ]
    let n_freq = ib.saturating_sub(ia) + 1;
    let n_lag = (2 * JZ + 1) as usize;
    let mut sync2d = vec![0.0f32; n_freq * n_lag];

    let idx = |fi: usize, lag: i32| fi * n_lag + (lag + JZ) as usize;

    for (fi, i) in (ia..=ib).enumerate() {
        for lag in -JZ..=JZ {
            let mut ta = 0.0f32;
            let mut tb = 0.0f32;
            let mut tc = 0.0f32;
            let mut t0a = 0.0f32;
            let mut t0b = 0.0f32;
            let mut t0c = 0.0f32;

            for n in 0usize..7 {
                let m = lag + JSTRT + (NSSY * n) as i32;
                let m36 = m + (NSSY * 36) as i32;
                let m72 = m + (NSSY * 72) as i32;

                let tone_bin = i + NFOS * COSTAS[n];

                // First Costas array
                if m >= 0 && (m as usize) < NHSYM {
                    let m = m as usize;
                    if tone_bin < NH1 {
                        ta += s[tone_bin][m];
                        t0a += (0..7)
                            .map(|k| s[(i + NFOS * k).min(NH1 - 1)][m])
                            .sum::<f32>();
                    }
                }
                // Second Costas array (+36 symbols)
                if m36 >= 0 && (m36 as usize) < NHSYM {
                    let m36 = m36 as usize;
                    if tone_bin < NH1 {
                        tb += s[tone_bin][m36];
                        t0b += (0..7)
                            .map(|k| s[(i + NFOS * k).min(NH1 - 1)][m36])
                            .sum::<f32>();
                    }
                }
                // Third Costas array (+72 symbols)
                if m72 >= 0 && (m72 as usize) < NHSYM {
                    let m72 = m72 as usize;
                    if tone_bin < NH1 {
                        tc += s[tone_bin][m72];
                        t0c += (0..7)
                            .map(|k| s[(i + NFOS * k).min(NH1 - 1)][m72])
                            .sum::<f32>();
                    }
                }
            }

            // Normalised scores (Fortran lines 75-83)
            let t = ta + tb + tc;
            let t0_abc = (t0a + t0b + t0c - t) / 6.0;
            let sync_abc = if t0_abc > 0.0 { t / t0_abc } else { 0.0 };

            let t_bc = tb + tc;
            let t0_bc = (t0b + t0c - t_bc) / 6.0;
            let sync_bc = if t0_bc > 0.0 { t_bc / t0_bc } else { 0.0 };

            sync2d[idx(fi, lag)] = sync_abc.max(sync_bc);
        }
    }

    // ── per-frequency peak detection ────────────────────────────────────��
    const MLAG: i32 = 10;
    let mut red = vec![0.0f32; n_freq];
    let mut red2 = vec![0.0f32; n_freq];
    let mut jpeak = vec![0i32; n_freq];
    let mut jpeak2 = vec![0i32; n_freq];

    for fi in 0..n_freq {
        // Narrow window peak (±MLAG)
        let (jp, rv) = (-MLAG..=MLAG)
            .map(|lag| (lag, sync2d[idx(fi, lag)]))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap_or((0, 0.0));
        jpeak[fi] = jp;
        red[fi] = rv;

        // Wide window peak (±JZ)
        let (jp2, rv2) = (-JZ..=JZ)
            .map(|lag| (lag, sync2d[idx(fi, lag)]))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap_or((0, 0.0));
        jpeak2[fi] = jp2;
        red2[fi] = rv2;
    }

    // Normalise by 40th-percentile noise floor
    let base = {
        let mut sorted = red.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pct_idx = (0.40 * n_freq as f32) as usize;
        sorted[pct_idx.min(n_freq - 1)].max(f32::EPSILON)
    };
    let base2 = {
        let mut sorted = red2.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pct_idx = (0.40 * n_freq as f32) as usize;
        sorted[pct_idx.min(n_freq - 1)].max(f32::EPSILON)
    };

    for r in red.iter_mut() { *r /= base; }
    for r in red2.iter_mut() { *r /= base2; }

    // ── collect candidates ────────────────────────────────────────────────
    let mut cands: Vec<SyncCandidate> = Vec::new();

    // Sort freq indices by narrow-window score (descending)
    let mut order: Vec<usize> = (0..n_freq).collect();
    order.sort_by(|&a, &b| red[b].partial_cmp(&red[a]).unwrap());

    for fi in order {
        if cands.len() >= max_cand * 2 {
            break;
        }
        let i = ia + fi;

        // Narrow peak
        if red[fi] >= sync_min && !red[fi].is_nan() {
            cands.push(SyncCandidate {
                freq_hz: i as f32 * DF,
                dt_sec: (jpeak[fi] as f32 - 0.5) * TSTEP,
                score: red[fi],
            });
        }
        // Wide peak (if different from narrow)
        if jpeak2[fi] != jpeak[fi] && red2[fi] >= sync_min && !red2[fi].is_nan() {
            cands.push(SyncCandidate {
                freq_hz: i as f32 * DF,
                dt_sec: (jpeak2[fi] as f32 - 0.5) * TSTEP,
                score: red2[fi],
            });
        }
    }

    // Remove near-duplicates (within 4 Hz and 40 ms)
    for i in 1..cands.len() {
        for j in 0..i {
            let fdiff = (cands[i].freq_hz - cands[j].freq_hz).abs();
            let tdiff = (cands[i].dt_sec - cands[j].dt_sec).abs();
            if fdiff < 4.0 && tdiff < 0.04 {
                if cands[i].score >= cands[j].score {
                    cands[j].score = 0.0;
                } else {
                    cands[i].score = 0.0;
                }
            }
        }
    }
    cands.retain(|c| c.score >= sync_min);

    // Sort: freq_hint matches first, then by descending score
    if let Some(fhint) = freq_hint {
        cands.sort_by(|a, b| {
            let a_near = (a.freq_hz - fhint).abs() <= 10.0;
            let b_near = (b.freq_hz - fhint).abs() <= 10.0;
            match (a_near, b_near) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => b.score.partial_cmp(&a.score).unwrap(),
            }
        });
    } else {
        cands.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    }

    cands.truncate(max_cand);
    cands
}

// ────────────────────────────────────────────────────────────────────────────
// Fine sync (sync8d)

/// Compute sync power for a downsampled complex FT8 signal.
///
/// `cd0` — complex samples at 200 Hz (output of `downsample`)
/// `i0`  — sample index of the first symbol start (0-based) in `cd0`
///
/// Returns the sum of Costas correlation powers across all three arrays.
/// Equivalent to WSJT-X sync8d.f90.
pub fn fine_sync_power(cd0: &[Complex<f32>], i0: usize) -> f32 {
    #[allow(dead_code)]
    const SPB: usize = 32; // downsampled samples per symbol (DS_SPB)

    // Pre-compute Costas reference waveforms (one per tone in the pattern)
    let csync: Vec<[Complex<f32>; 32]> = COSTAS
        .iter()
        .map(|&tone| {
            let dphi = 2.0 * PI * tone as f32 / SPB as f32;
            let mut waves = [Complex::new(0.0f32, 0.0); 32];
            let mut phi = 0.0f32;
            for w in waves.iter_mut() {
                *w = Complex::new(phi.cos(), phi.sin());
                phi = (phi + dphi) % (2.0 * PI);
            }
            waves
        })
        .collect();

    let mut sync = 0.0f32;
    let np2 = cd0.len();

    for (idx_cos, ref_tone) in csync.iter().enumerate() {
        let i1 = i0 + idx_cos * SPB;
        let i2 = i1 + 36 * SPB;
        let i3 = i1 + 72 * SPB;

        let correlate = |start: usize| -> f32 {
            if start + SPB <= np2 {
                cd0[start..start + SPB]
                    .iter()
                    .zip(ref_tone.iter())
                    .map(|(&s, &r)| s * r.conj())
                    .sum::<Complex<f32>>()
                    .norm_sqr()
            } else {
                0.0
            }
        };

        sync += correlate(i1) + correlate(i2) + correlate(i3);
    }

    sync
}

/// Parabolic interpolation refinement on three adjacent values.
/// Returns the sub-sample offset in [-0.5, +0.5] and the interpolated peak value.
pub fn parabolic_peak(y_neg: f32, y_0: f32, y_pos: f32) -> (f32, f32) {
    let denom = y_neg - 2.0 * y_0 + y_pos;
    if denom.abs() < f32::EPSILON {
        return (0.0, y_0);
    }
    let offset = 0.5 * (y_neg - y_pos) / denom;
    let peak = y_0 - 0.25 * (y_neg - y_pos) * offset;
    (offset.clamp(-0.5, 0.5), peak)
}

/// Refine a coarse candidate using fine sync and optional parabolic interpolation.
///
/// Scans ±`search_steps` samples around `candidate.dt_sec` at the downsampled
/// rate (200 Hz) and returns the refined candidate with the best sync power.
pub fn refine_candidate(
    cd0: &[Complex<f32>],
    candidate: &SyncCandidate,
    search_steps: i32,
) -> SyncCandidate {
    const SPB: usize = 32;
    let nominal_i0 = (candidate.dt_sec * 200.0).round() as i32;

    let (best_i0, best_score) = (-search_steps..=search_steps)
        .map(|delta| {
            let i0 = (nominal_i0 + delta).max(0) as usize;
            let score = fine_sync_power(cd0, i0);
            (i0, score)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap_or((0, 0.0));

    SyncCandidate {
        freq_hz: candidate.freq_hz,
        dt_sec: best_i0 as f32 / 200.0,
        score: best_score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parabolic_peak_at_center() {
        // Symmetric peak: offset should be 0
        let (offset, _) = parabolic_peak(1.0, 2.0, 1.0);
        assert!((offset).abs() < 1e-6, "expected offset 0, got {offset}");
    }

    #[test]
    fn parabolic_peak_offset_right() {
        // Asymmetric: y(+1) > y(-1), peak should shift right
        let (offset, _) = parabolic_peak(0.5, 1.5, 2.0);
        assert!(offset > 0.0, "expected positive offset for right-skewed peak");
    }

    #[test]
    fn fine_sync_silence_is_zero() {
        let cd0 = vec![Complex::new(0.0f32, 0.0); 3200];
        let sync = fine_sync_power(&cd0, 0);
        assert_eq!(sync, 0.0);
    }

    #[test]
    fn coarse_sync_on_silence_returns_empty_or_low() {
        let audio = vec![0i16; 15 * 12000];
        let cands = coarse_sync(&audio, 200.0, 2800.0, 1.0, None, 100);
        // Silence may return 0 candidates or candidates with low score
        // Just check it doesn't panic and returns a bounded list
        assert!(cands.len() <= 100);
    }
}
