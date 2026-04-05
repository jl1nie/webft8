// SPDX-License-Identifier: GPL-3.0-or-later
//! Butterworth bandpass filter for simulating hardware CW/SSB narrow filters.
//!
//! Implements a **2N-th order** Butterworth BPF as a cascade of N biquad
//! sections (second-order IIR, Direct Form II Transposed).
//!
//! Design route:
//! 1. Compute Nth-order Butterworth lowpass prototype poles.
//! 2. Apply the LP→BP frequency transform  s → (s² + ω₀²) / (Bs).
//! 3. Map each analog BP pole to digital via the bilinear transform.
//! 4. Normalise gain to unity at the geometric centre frequency.

use std::f64::consts::PI;

// ────────────────────────────────────────────────────────────────────────────
// Biquad section

/// Second-order IIR section (Direct Form II Transposed).
#[derive(Clone, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    // transposed state
    s1: f64,
    s2: f64,
}

impl Biquad {
    fn new(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Self {
        Biquad { b0, b1, b2, a1, a2, s1: 0.0, s2: 0.0 }
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        y
    }

    /// Evaluate magnitude of H(e^{jω}).
    fn mag_at(&self, w: f64) -> f64 {
        let c1 = w.cos();
        let s1 = w.sin();
        let c2 = (2.0 * w).cos();
        let s2 = (2.0 * w).sin();

        let nr = self.b0 * c2 + self.b1 * c1 + self.b2;
        let ni = self.b0 * s2 + self.b1 * s1;
        let dr = c2 + self.a1 * c1 + self.a2;
        let di = s2 + self.a1 * s1;

        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Public filter struct

/// Butterworth bandpass filter (cascade of biquad sections).
pub struct ButterworthBpf {
    sections: Vec<Biquad>,
}

impl ButterworthBpf {
    /// Design a Butterworth BPF.
    ///
    /// * `n_poles` — lowpass prototype order (total BPF order = 2 × n_poles).
    ///   Use 4 for an 8th-order filter (typical crystal CW filter).
    /// * `f_low`, `f_high` — −3 dB passband edge frequencies (Hz).
    /// * `fs` — sample rate (Hz).
    pub fn design(n_poles: usize, f_low: f64, f_high: f64, fs: f64) -> Self {
        assert!(n_poles >= 1);
        assert!(0.0 < f_low && f_low < f_high && f_high < fs / 2.0);

        let t = 1.0 / (2.0 * fs); // half period for bilinear

        // Pre-warp analog edge frequencies
        let wl = (PI * f_low / fs).tan() / t;
        let wh = (PI * f_high / fs).tan() / t;
        let w0sq = wl * wh;
        let bw = wh - wl;

        let half = n_poles / 2;
        let mut sections = Vec::with_capacity(n_poles);

        // Conjugate LP pole pairs → 2 biquads each
        for k in 0..half {
            let theta =
                PI * (2.0 * k as f64 + n_poles as f64 + 1.0) / (2.0 * n_poles as f64);
            let p_re = theta.cos();
            let p_im = theta.sin();

            for (s_re, s_im) in lp_to_bp(p_re, p_im, bw, w0sq) {
                let (z_re, z_im) = bilinear(s_re, s_im, t);
                sections.push(biquad_from_pole(z_re, z_im));
            }
        }

        // Odd-order: real LP pole at s = −1
        if n_poles % 2 == 1 {
            let poles = lp_to_bp(-1.0, 0.0, bw, w0sq);
            // For a narrow BPF, the discriminant is negative → conjugate pair → 1 biquad
            let (s_re, s_im) = poles[0];
            let (z_re, z_im) = bilinear(s_re, s_im, t);
            sections.push(biquad_from_pole(z_re, z_im));
        }

        // Normalise to unity gain at geometric centre
        let fc = (f_low * f_high).sqrt();
        let wc = 2.0 * PI * fc / fs;
        let gain: f64 = sections.iter().map(|s| s.mag_at(wc)).product();
        if let Some(sec) = sections.first_mut() {
            let g = 1.0 / gain;
            sec.b0 *= g;
            sec.b2 *= g;
        }

        ButterworthBpf { sections }
    }

    /// Filter a block of f32 samples, returning a new Vec.
    pub fn filter(&mut self, input: &[f32]) -> Vec<f32> {
        input
            .iter()
            .map(|&x| {
                let mut y = x as f64;
                for sec in &mut self.sections {
                    y = sec.tick(y);
                }
                y as f32
            })
            .collect()
    }

    /// Magnitude response |H(f)| in linear scale.
    pub fn response_db(&self, f: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let mag: f64 = self.sections.iter().map(|s| s.mag_at(w)).product();
        20.0 * mag.log10()
    }

    /// Number of biquad sections.
    pub fn order(&self) -> usize {
        self.sections.len() * 2
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Internal helpers

/// LP → BP transform: each LP pole generates 2 analog BP poles.
fn lp_to_bp(p_re: f64, p_im: f64, bw: f64, w0sq: f64) -> Vec<(f64, f64)> {
    let pbw_re = p_re * bw;
    let pbw_im = p_im * bw;

    // discriminant = (p·bw)² − 4·ω₀²
    let d_re = pbw_re * pbw_re - pbw_im * pbw_im - 4.0 * w0sq;
    let d_im = 2.0 * pbw_re * pbw_im;

    let (sd_re, sd_im) = csqrt(d_re, d_im);

    vec![
        ((pbw_re + sd_re) / 2.0, (pbw_im + sd_im) / 2.0),
        ((pbw_re - sd_re) / 2.0, (pbw_im - sd_im) / 2.0),
    ]
}

/// Complex square root.
fn csqrt(re: f64, im: f64) -> (f64, f64) {
    let r = (re * re + im * im).sqrt();
    let theta = im.atan2(re);
    let sr = r.sqrt();
    (sr * (theta / 2.0).cos(), sr * (theta / 2.0).sin())
}

/// Bilinear transform: analog pole s → digital pole z.
fn bilinear(s_re: f64, s_im: f64, t: f64) -> (f64, f64) {
    let nr = 1.0 + s_re * t;
    let ni = s_im * t;
    let dr = 1.0 - s_re * t;
    let di = -s_im * t;
    let d_sq = dr * dr + di * di;
    ((nr * dr + ni * di) / d_sq, (ni * dr - nr * di) / d_sq)
}

/// Build a biquad section from a digital pole and its implied conjugate.
///
/// Numerator = z² − 1 (BPF zeros at z = ±1).
/// Denominator = z² − 2·Re(z_pole)·z + |z_pole|².
fn biquad_from_pole(z_re: f64, z_im: f64) -> Biquad {
    Biquad::new(
        1.0,
        0.0,
        -1.0,
        -2.0 * z_re,
        z_re * z_re + z_im * z_im,
    )
}

// ────────────────────────────────────────────────────────────────────────────
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unity_gain_at_center() {
        let bpf = ButterworthBpf::design(4, 750.0, 1250.0, 12_000.0);
        let fc = (750.0_f64 * 1250.0).sqrt();
        let db = bpf.response_db(fc, 12_000.0);
        assert!(
            db.abs() < 0.1,
            "gain at centre should be ~0 dB, got {db:.2} dB"
        );
    }

    #[test]
    fn passband_edges_near_minus_3db() {
        let bpf = ButterworthBpf::design(4, 750.0, 1250.0, 12_000.0);
        let db_low = bpf.response_db(750.0, 12_000.0);
        let db_high = bpf.response_db(1250.0, 12_000.0);
        // Butterworth definition: −3 dB at the passband edges
        assert!(
            (db_low - (-3.0)).abs() < 0.5,
            "low edge: expected ~−3 dB, got {db_low:.2} dB"
        );
        assert!(
            (db_high - (-3.0)).abs() < 0.5,
            "high edge: expected ~−3 dB, got {db_high:.2} dB"
        );
    }

    #[test]
    fn steep_roll_off_outside_passband() {
        let bpf = ButterworthBpf::design(4, 750.0, 1250.0, 12_000.0);
        // At 500 Hz (250 Hz below passband) and 1500 Hz (250 Hz above)
        let db_below = bpf.response_db(500.0, 12_000.0);
        let db_above = bpf.response_db(1500.0, 12_000.0);
        assert!(
            db_below < -15.0,
            "500 Hz should be well attenuated, got {db_below:.1} dB"
        );
        assert!(
            db_above < -15.0,
            "1500 Hz should be well attenuated, got {db_above:.1} dB"
        );
    }

    #[test]
    fn filter_passes_center_tone() {
        let mut bpf = ButterworthBpf::design(4, 750.0, 1250.0, 12_000.0);
        let fs = 12_000.0;
        let n = 12_000usize; // 1 second
        let f = 1000.0; // center
        let input: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / fs as f32).sin())
            .collect();
        let output = bpf.filter(&input);
        // After settling (skip first 2000 samples), amplitude should be close to 1.0
        let peak = output[2000..].iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            (peak - 1.0).abs() < 0.05,
            "center tone peak should be ~1.0, got {peak:.3}"
        );
    }

    #[test]
    fn filter_attenuates_out_of_band() {
        let mut bpf = ButterworthBpf::design(4, 750.0, 1250.0, 12_000.0);
        let fs = 12_000.0;
        let n = 12_000usize;
        let f = 2000.0; // well outside passband
        let input: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / fs as f32).sin())
            .collect();
        let output = bpf.filter(&input);
        let peak = output[2000..].iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak < 0.05,
            "out-of-band tone should be heavily attenuated, got {peak:.4}"
        );
    }
}
