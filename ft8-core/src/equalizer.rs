// SPDX-License-Identifier: GPL-3.0-or-later
//! Adaptive equalizer for FT8 signals distorted by a hardware BPF.
//!
//! The 500 Hz hardware bandpass filter introduces amplitude roll-off and phase
//! rotation at the passband edges.  This module estimates the channel transfer
//! function H(f) using the known Costas-array pilot tones and applies
//! H⁻¹(f) to flatten the 8-tone symbol spectra before LLR computation.
//!
//! ## Per-signal (local) equalization
//!
//! Each FT8 frame contains three Costas arrays (positions 0–6, 36–42, 72–78)
//! that use the pattern `[3,1,4,0,6,5,2]`, visiting tones 0–6 once each.
//! By averaging the received complex amplitude at each pilot tone across
//! the three arrays, we obtain 7 samples of A·H(f).  Tone 7 (never visited
//! by Costas) is linearly extrapolated from tones 5 and 6.
//!
//! A zero-forcing weight is computed:
//!
//! ```text
//! W[t] = pilot[t]* / (|pilot[t]|² + ε)
//! ```
//!
//! and applied to every symbol: `cs_eq[sym][t] = cs[sym][t] · W[t]`.

use num_complex::Complex;
use crate::params::COSTAS_POS;

// ────────────────────────────────────────────────────────────────────────────
// Public types

/// Equalizer operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqMode {
    /// No equalization (passthrough).
    Off,
    /// Per-signal equalization using local Costas arrays.
    Local,
    /// Try without EQ first; fall back to EQ only if BP decode fails.
    /// Avoids center-case degradation while recovering edge-case signals.
    Adaptive,
}

// ────────────────────────────────────────────────────────────────────────────
// Core implementation

/// Reverse lookup: for each tone 0–6, the position within the Costas pattern.
///
/// `COSTAS = [3,1,4,0,6,5,2]`  →  tone 0 is at index 3, tone 1 at index 1, ��
const COSTAS_REV: [usize; 7] = [3, 1, 6, 0, 2, 5, 4];

/// Estimate the channel response H(f) at each of the 8 tone bins from the
/// three Costas arrays in the symbol spectra.
///
/// Returns 8 complex pilot estimates.  Tones 0–6 are averaged across the
/// three arrays; tone 7 is linearly extrapolated from tones 5 and 6.
fn estimate_pilots(cs: &[[Complex<f32>; 8]; 79]) -> [Complex<f32>; 8] {
    let mut pilots = [Complex::new(0.0f32, 0.0); 8];

    // Tones 0–6: average across 3 Costas arrays
    for tone in 0..7 {
        let k = COSTAS_REV[tone]; // position within the 7-symbol Costas block
        let mut sum = Complex::new(0.0f32, 0.0);
        for &offset in &COSTAS_POS {
            sum += cs[offset + k][tone];
        }
        pilots[tone] = sum / 3.0;
    }

    // Tone 7: linear extrapolation from tones 5 and 6
    // H(7) ≈ 2·H(6) − H(5)
    pilots[7] = pilots[6] * 2.0 - pilots[5];

    pilots
}

/// Apply per-signal (local) equalization to the symbol spectra in-place.
///
/// Uses the Costas pilot tones to estimate H(f) and applies a Wiener-style
/// correction `W[t] = pilot*/(|pilot|² + σ²_noise)`.  The noise variance
/// `σ²_noise` is estimated from the scatter of the 3 per-array Costas
/// observations, so the equalizer is self-regulating:
///
/// * **High SNR → σ² small → full correction** (beneficial at BPF edge).
/// * **Low SNR → σ² large → conservative / near-passthrough** (avoids noise
///   amplification when the channel estimate is unreliable).
///
/// The overall gain is normalised to preserve downstream SNR estimation.
pub fn equalize_local(cs: &mut [[Complex<f32>; 8]; 79]) {
    let pilots = estimate_pilots(cs);

    // Estimate noise variance from scatter of the 3 per-array pilot observations.
    // For each tone 0–6, compute variance across the 3 Costas arrays.
    let noise_var = {
        let mut total_var = 0.0f32;
        let mut count = 0usize;
        for tone in 0..7 {
            let k = COSTAS_REV[tone];
            let mut obs = [Complex::new(0.0f32, 0.0); 3];
            for (i, &offset) in COSTAS_POS.iter().enumerate() {
                obs[i] = cs[offset + k][tone];
            }
            let mean = pilots[tone];
            for o in &obs {
                total_var += (*o - mean).norm_sqr();
                count += 1;
            }
        }
        if count > 0 { total_var / count as f32 } else { 1.0 }
    };

    // Wiener regularisation: use the larger of observed noise variance and
    // a fraction of median pilot power.  This ensures near-passthrough at low
    // SNR (where pilot estimates are unreliable) while allowing full correction
    // at high SNR.
    let mut powers: Vec<f32> = pilots.iter().map(|p| p.norm_sqr()).collect();
    powers.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_power = powers[powers.len() / 2];
    let noise_var = noise_var.max(median_power * 0.3);

    // Wiener weights: W[t] = pilot[t]* / (|pilot[t]|² + σ²_noise)
    let mut weights: [Complex<f32>; 8] = [Complex::new(0.0, 0.0); 8];
    for t in 0..8 {
        let p = pilots[t];
        weights[t] = p.conj() / (p.norm_sqr() + noise_var);
    }

    // Normalise: mean |W[t]| = 1 (preserves signal level)
    let mean_mag = weights.iter().map(|w| w.norm()).sum::<f32>() / 8.0;
    if mean_mag > f32::EPSILON {
        for w in weights.iter_mut() {
            *w /= mean_mag;
        }
    }

    // Apply to all 79 symbols
    for sym in cs.iter_mut() {
        for t in 0..8 {
            sym[t] *= weights[t];
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::COSTAS;
    use std::f32::consts::PI;

    /// With a flat channel (all tones equal), equalization should be a no-op.
    #[test]
    fn flat_channel_is_noop() {
        let mut cs = [[Complex::new(0.0f32, 0.0); 8]; 79];
        // Fill Costas positions with uniform amplitude
        for &offset in &COSTAS_POS {
            for (k, &tone) in COSTAS.iter().enumerate() {
                cs[offset + k][tone] = Complex::new(1.0, 0.0);
            }
        }
        // Fill data symbols
        for sym in 0..79 {
            for t in 0..8 {
                if cs[sym][t] == Complex::new(0.0, 0.0) {
                    cs[sym][t] = Complex::new(1.0, 0.0);
                }
            }
        }

        let orig = cs;
        equalize_local(&mut cs);

        // After equalization, amplitudes should be approximately preserved
        for sym in 0..79 {
            for t in 0..8 {
                let ratio = cs[sym][t].norm() / orig[sym][t].norm().max(1e-10);
                assert!(
                    (ratio - 1.0).abs() < 0.1,
                    "sym={sym} t={t}: ratio={ratio:.3}"
                );
            }
        }
    }

    /// Simulated BPF edge: tones 5,6,7 attenuated.  Equalizer should reduce
    /// the amplitude spread (not perfectly flatten, due to Wiener regularisation).
    #[test]
    fn edge_attenuation_corrected() {
        let mut cs = [[Complex::new(0.0f32, 0.0); 8]; 79];

        // BPF-like response: tones 0-4 @ 1.0, tone 5 @ 0.7, tone 6 @ 0.5, tone 7 @ 0.3
        let h: [f32; 8] = [1.0, 1.0, 1.0, 1.0, 1.0, 0.7, 0.5, 0.3];

        for sym in 0..79 {
            for t in 0..8 {
                cs[sym][t] = Complex::new(h[t], 0.0);
            }
        }

        // Measure spread before
        let mags_before: Vec<f32> = (0..8).map(|t| cs[40][t].norm()).collect();
        let mean_before = mags_before.iter().sum::<f32>() / 8.0;
        let cv_before = {
            let v = mags_before.iter().map(|&m| (m - mean_before).powi(2)).sum::<f32>() / 8.0;
            v.sqrt() / mean_before
        };

        equalize_local(&mut cs);

        // Measure spread after
        let mags_after: Vec<f32> = (0..8).map(|t| cs[40][t].norm()).collect();
        let mean_after = mags_after.iter().sum::<f32>() / 8.0;
        let cv_after = {
            let v = mags_after.iter().map(|&m| (m - mean_after).powi(2)).sum::<f32>() / 8.0;
            v.sqrt() / mean_after
        };

        assert!(
            cv_after < cv_before,
            "EQ should reduce amplitude spread: CV before={cv_before:.3}, after={cv_after:.3}"
        );
    }

    /// Phase distortion: tones have different phases.  Equalizer should align.
    #[test]
    fn phase_distortion_corrected() {
        let mut cs = [[Complex::new(0.0f32, 0.0); 8]; 79];

        // Each tone has a different phase shift (simulating group delay)
        let phases: [f32; 8] = [0.0, 0.1, 0.2, 0.3, 0.5, 0.8, 1.2, 1.6];

        for sym in 0..79 {
            for t in 0..8 {
                let mag = 1.0;
                cs[sym][t] = Complex::new(
                    mag * phases[t].cos(),
                    mag * phases[t].sin(),
                );
            }
        }

        equalize_local(&mut cs);

        // After equalization, phases should be approximately aligned
        // (all relative to tone 0)
        let ref_phase = cs[40][0].arg();
        for t in 1..7 {
            let phase_diff = (cs[40][t].arg() - ref_phase).abs();
            let phase_diff = phase_diff.min(2.0 * PI - phase_diff);
            assert!(
                phase_diff < 0.15,
                "tone {t}: phase diff={phase_diff:.3} rad"
            );
        }
    }
}
