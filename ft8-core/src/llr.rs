/// Soft-decision LLR computation for FT8.
/// Ported from WSJT-X ft8b.f90 lines 154-239.
///
/// Pipeline:
///   1. Extract 32-sample complex windows for each of the 79 symbols
///   2. 32-point FFT → 8 complex tone bins per symbol
///   3. Compute bit-metric (bm) using Gray-coded soft decisions
///   4. Normalise by standard deviation
///   5. Scale by 2.83
use num_complex::Complex;
use rustfft::FftPlanner;

use crate::params::{GRAYMAP, LDPC_N, LLR_SCALE};

// ────────────────────────────────────────────────────────────────────────────
// Helpers

/// Normalise a metric vector by its standard deviation.
/// Equivalent to WSJT-X `normalizebmet`.
fn normalize_bmet(bmet: &mut [f32]) {
    let n = bmet.len() as f32;
    let mean = bmet.iter().sum::<f32>() / n;
    let mean_sq = bmet.iter().map(|&x| x * x).sum::<f32>() / n;
    let var = mean_sq - mean * mean;
    let sig = if var > 0.0 { var.sqrt() } else { mean_sq.sqrt() };
    if sig > 0.0 {
        bmet.iter_mut().for_each(|x| *x /= sig);
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Symbol spectra

/// Compute 8-tone complex spectra for all 79 FT8 symbols.
///
/// `cd0`   — downsampled complex signal at 200 Hz (output of `downsample`)
/// `i_start` — sample index in `cd0` of the first symbol (from fine sync)
///
/// Returns `cs[symbol][tone]` (79 × 8 complex values) scaled by 1/1000.
pub fn symbol_spectra(
    cd0: &[Complex<f32>],
    i_start: usize,
) -> Box<[[Complex<f32>; 8]; 79]> {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(32);
    // SAFETY: array of Copy type, zero-initialised on heap
    let mut cs: Box<[[Complex<f32>; 8]; 79]> =
        vec![[Complex::new(0.0, 0.0); 8]; 79].try_into().unwrap();

    let mut buf = vec![Complex::new(0.0f32, 0.0); 32];
    for (k, cs_k) in cs.iter_mut().enumerate() {
        let i1 = i_start + k * 32;
        // Fill buffer; out-of-bounds regions stay at zero
        for (j, b) in buf.iter_mut().enumerate() {
            *b = if i1 + j < cd0.len() { cd0[i1 + j] } else { Complex::new(0.0, 0.0) };
        }
        fft.process(&mut buf);
        for t in 0..8 {
            cs_k[t] = buf[t] / 1000.0;
        }
    }
    cs
}

// ────────────────────────────────────────────────────────────────────────────
// LLR computation

/// Compute four LLR vectors (a, b, c, d) from the 79-symbol complex spectra.
///
/// The four vectors correspond to grouping schemes nsym = 1, 2, 3 and an
/// additional bit-by-bit normalised variant (d), matching WSJT-X's
/// `bmeta`, `bmetb`, `bmetc`, `bmetd` and the final `llra`-`llrd` vectors.
///
/// All returned vectors have length LDPC_N = 174, in LDPC bit-index order.
pub struct LlrSet {
    /// nsym=1 soft metrics, scaled (matches WSJT-X llra)
    pub llra: [f32; LDPC_N],
    /// nsym=2 soft metrics, scaled (matches WSJT-X llrb)
    pub llrb: [f32; LDPC_N],
    /// nsym=3 soft metrics, scaled (matches WSJT-X llrc)
    pub llrc: [f32; LDPC_N],
    /// nsym=1 bit-normalised (matches WSJT-X llrd)
    pub llrd: [f32; LDPC_N],
}

/// Compute soft LLRs from complex symbol spectra.
pub fn compute_llr(cs: &[[Complex<f32>; 8]; 79]) -> LlrSet {
    let mut bmeta = [0.0f32; LDPC_N];
    let mut bmetb = [0.0f32; LDPC_N];
    let mut bmetc = [0.0f32; LDPC_N];
    let mut bmetd = [0.0f32; LDPC_N];

    for nsym in 1usize..=3 {
        let nt = 1usize << (3 * nsym); // 8, 64, 512
        let ibmax = 3 * nsym - 1;      // 2, 5, 8

        // Precompute |s2| for all nt combinations for the current block.
        let mut s2 = vec![0.0f32; nt];

        for ihalf in 0..2usize {
            // Step by nsym through 29 data blocks per half
            let mut k = 0usize;
            while k < 29 {
                // Symbol index (0-based): first half uses symbols 7..35, second 43..71
                let ks = k + if ihalf == 0 { 7 } else { 43 };

                // Compute metrics for all nt combinations
                for i in 0..nt {
                    let i1 = i / 64;
                    let i2 = (i & 63) / 8;
                    let i3 = i & 7;
                    s2[i] = match nsym {
                        1 => cs[ks][GRAYMAP[i3]].norm(),
                        2 => (cs[ks][GRAYMAP[i2]] + cs[ks + 1][GRAYMAP[i3]]).norm(),
                        3 => (cs[ks][GRAYMAP[i1]]
                            + cs[ks + 1][GRAYMAP[i2]]
                            + cs[ks + 2][GRAYMAP[i3]])
                            .norm(),
                        _ => unreachable!(),
                    };
                }

                // Bit index in the 174-bit LDPC word (0-based)
                let i_bit_base = k * 3 + ihalf * 87;

                for ib in 0..=ibmax {
                    let bit_idx = i_bit_base + ib;
                    if bit_idx >= LDPC_N {
                        break;
                    }
                    // Which bit of i selects 1 vs 0 for this position?
                    let bit_sel = ibmax - ib;

                    let max_one = s2
                        .iter()
                        .enumerate()
                        .filter(|&(i, _)| (i >> bit_sel) & 1 == 1)
                        .map(|(_, &v)| v)
                        .fold(f32::NEG_INFINITY, f32::max);

                    let max_zero = s2
                        .iter()
                        .enumerate()
                        .filter(|&(i, _)| (i >> bit_sel) & 1 == 0)
                        .map(|(_, &v)| v)
                        .fold(f32::NEG_INFINITY, f32::max);

                    let bm = max_one - max_zero;

                    match nsym {
                        1 => {
                            bmeta[bit_idx] = bm;
                            let den = max_one.max(max_zero);
                            bmetd[bit_idx] = if den > 0.0 { bm / den } else { 0.0 };
                        }
                        2 => bmetb[bit_idx] = bm,
                        3 => bmetc[bit_idx] = bm,
                        _ => unreachable!(),
                    }
                }

                k += nsym;
            }
        }
    }

    // Normalise by standard deviation and scale
    normalize_bmet(&mut bmeta);
    normalize_bmet(&mut bmetb);
    normalize_bmet(&mut bmetc);
    normalize_bmet(&mut bmetd);

    let scale = |v: &[f32; LDPC_N]| -> [f32; LDPC_N] {
        let mut out = [0.0f32; LDPC_N];
        for (o, &x) in out.iter_mut().zip(v.iter()) {
            *o = x * LLR_SCALE;
        }
        out
    };

    LlrSet {
        llra: scale(&bmeta),
        llrb: scale(&bmetb),
        llrc: scale(&bmetc),
        llrd: scale(&bmetd),
    }
}

/// Hard-decision sync quality check: count how many of the 21 Costas tones
/// are correctly decoded. Returns 0..21 (WSJT-X bails out if ≤ 6).
pub fn sync_quality(cs: &[[Complex<f32>; 8]; 79]) -> u32 {
    use crate::params::COSTAS;
    let mut count = 0u32;

    // Costas positions: symbols 0..7, 36..43, 72..79 (0-based)
    for (offset_idx, &sym_offset) in [0usize, 36, 72].iter().enumerate() {
        for t in 0..7 {
            let sym = sym_offset + t;
            let expected_tone = COSTAS[t];
            // Find tone with maximum amplitude
            let best_tone = cs[sym]
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.norm().partial_cmp(&b.1.norm()).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0);
            if best_tone == expected_tone {
                count += 1;
            }
        }
        let _ = offset_idx;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zero spectra input should produce near-zero LLRs (no signal).
    #[test]
    fn zero_spectra_zero_llr() {
        let cs = vec![[Complex::new(0.0f32, 0.0); 8]; 79]
            .try_into()
            .unwrap();
        let llr_set = compute_llr(&cs);
        let any_large = llr_set.llra.iter().any(|&x| x.abs() > 1.0);
        assert!(!any_large, "zero input should not produce large LLRs");
    }

    #[test]
    fn llr_length_is_174() {
        let cs = vec![[Complex::new(0.0f32, 0.0); 8]; 79]
            .try_into()
            .unwrap();
        let llr_set = compute_llr(&cs);
        assert_eq!(llr_set.llra.len(), 174);
        assert_eq!(llr_set.llrd.len(), 174);
    }

    #[test]
    fn sync_quality_costas_perfect() {
        // Build spectra where every Costas position has the correct dominant tone
        use crate::params::COSTAS;
        let mut cs = vec![[Complex::new(0.0f32, 0.0); 8]; 79];
        for &sym_offset in &[0usize, 36, 72] {
            for t in 0..7 {
                let sym = sym_offset + t;
                cs[sym][COSTAS[t]] = Complex::new(1.0, 0.0); // correct tone is brightest
            }
        }
        let cs_box: Box<[[Complex<f32>; 8]; 79]> = cs.try_into().unwrap();
        assert_eq!(sync_quality(&cs_box), 21);
    }
}
