/// FFT-based downsampling from 12 000 Hz to 200 Hz.
/// Ported from WSJT-X ft8_downsample.f90.
///
/// Algorithm (matching the Fortran):
///   1. Zero-pad 180 000 real samples → 192 000
///   2. Forward FFT (complex, treating real input as imaginary=0)
///   3. Extract positive-frequency bins covering [f0-9.375, f0+53.125] Hz
///   4. Apply Hann taper to the 101 edge bins on each side
///   5. Rotate left by (i0 - ib) to place f0 at DC (bin 0)
///   6. Inverse 3200-point complex FFT
///   7. Scale by 1/sqrt(NFFT1 × NFFT2)
///
/// Output: 3200 complex samples at 200 Hz (32 samples per FT8 symbol).
use num_complex::Complex;
use rustfft::FftPlanner;

#[allow(dead_code)]
const NMAX: usize = 15 * 12_000; // 180 000 — receive buffer length
const NFFT1: usize = 192_000;    // zero-padded FFT size
const NFFT2: usize = 3_200;      // downsampled output size (200 Hz)
const DF: f32 = 12_000.0 / NFFT1 as f32; // 0.0625 Hz per bin
const BAUD: f32 = 12_000.0 / 1_920.0;    // 6.25 Hz

/// Downconvert and decimate `audio` (16-bit PCM, 12 000 Hz, ≤ 180 000 samples)
/// to a complex baseband signal at 200 Hz centred on `f0` (Hz).
///
/// Returns 3200 complex samples.  The caller may pass `fft_cache` (output of a
/// previous forward-FFT call on the same audio block) to skip the expensive
/// 192 000-point FFT when only `f0` changes.
pub fn downsample(
    audio: &[i16],
    f0: f32,
    fft_cache: Option<&[Complex<f32>]>,
) -> (Vec<Complex<f32>>, Vec<Complex<f32>>) {
    let mut planner = FftPlanner::<f32>::new();

    // --- Step 1 & 2: large forward FFT (cached or fresh) ---
    let cx: Vec<Complex<f32>> = if let Some(cache) = fft_cache {
        cache.to_vec()
    } else {
        let mut x: Vec<Complex<f32>> = audio
            .iter()
            .map(|&s| Complex::new(s as f32, 0.0))
            .chain(std::iter::repeat(Complex::new(0.0, 0.0)))
            .take(NFFT1)
            .collect();
        let fft1 = planner.plan_fft_forward(NFFT1);
        fft1.process(&mut x);
        x
    };

    // --- Step 3: extract frequency band ---
    let i0 = (f0 / DF).round() as usize;
    let ft = f0 + 8.5 * BAUD;
    let fb = f0 - 1.5 * BAUD;
    let it = ((ft / DF).round() as usize).min(NFFT1 / 2);
    let ib = ((fb / DF).round() as usize).max(1);
    let k = it - ib + 1; // number of extracted bins (≈ 1001)

    let mut c1 = vec![Complex::new(0.0f32, 0.0); NFFT2];
    for (dst, src) in c1[..k].iter_mut().zip(cx[ib..=it].iter()) {
        *dst = *src;
    }

    // --- Step 4: Hann taper on leading / trailing 101 bins ---
    // taper[i] = 0.5 * (1 + cos(i*π/100)),  i = 0..=100
    // taper[0]=1.0, taper[100]=0.0  (one end of a raised-cosine window)
    let taper: Vec<f32> = (0..=100_usize)
        .map(|i| 0.5 * (1.0 + (i as f32 * std::f32::consts::PI / 100.0).cos()))
        .collect();

    // Leading edge: multiply c1[0..=100] by taper[100..=0] (ramp up 0→1)
    for i in 0..=100 {
        c1[i] *= taper[100 - i];
    }
    // Trailing edge: multiply c1[k-101..=k-1] by taper[0..=100] (ramp down 1→0)
    if k > 100 {
        for i in 0..=100 {
            c1[k - 101 + i] *= taper[i];
        }
    }

    // --- Step 5: cyclic shift — place f0 at DC (bin 0) ---
    // Fortran: c1 = cshift(c1, i0-ib)  (rotate-left by i0-ib over the full array)
    let shift = (i0.saturating_sub(ib)) % NFFT2;
    c1.rotate_left(shift);

    // --- Step 6: inverse 3200-point complex FFT ---
    let fft2 = planner.plan_fft_inverse(NFFT2);
    fft2.process(&mut c1);

    // --- Step 7: combined scale factor ---
    let fac = 1.0 / ((NFFT1 as f32) * (NFFT2 as f32)).sqrt();
    for s in c1.iter_mut() {
        *s *= fac;
    }

    (c1, cx) // return both result and FFT cache
}

/// Convenience wrapper: no cache, returns only the 3200-sample baseband signal.
pub fn downsample_simple(audio: &[i16], f0: f32) -> Vec<Complex<f32>> {
    downsample(audio, f0, None).0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine wave at f0 should produce a signal whose energy concentrates near DC
    /// in the output spectrum (most power within ±10 bins of DC).
    #[test]
    fn sine_at_f0_energy_at_dc() {
        let f0 = 1000.0f32;
        let audio: Vec<i16> = (0..NMAX)
            .map(|n| {
                let t = n as f32 / 12_000.0;
                (10_000.0 * (2.0 * std::f32::consts::PI * f0 * t).sin()) as i16
            })
            .collect();

        let out = downsample_simple(&audio, f0);

        // Take FFT of the output and check energy distribution.
        let mut spectrum = out.clone();
        let mut planner = rustfft::FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(NFFT2);
        fft.process(&mut spectrum);

        // Energy near DC (bins 0..=10 and the aliased negative side 3190..3199)
        let energy_near_dc: f32 = spectrum[..=10]
            .iter()
            .chain(spectrum[NFFT2 - 10..].iter())
            .map(|c| c.norm_sqr())
            .sum();

        let total_energy: f32 = spectrum.iter().map(|c| c.norm_sqr()).sum();

        assert!(
            total_energy > 0.0,
            "total energy should be non-zero"
        );
        let frac = energy_near_dc / total_energy;
        assert!(
            frac > 0.5,
            "energy near DC fraction = {frac:.3} (expected > 50%)"
        );
    }

    /// A sine wave at f0+100 Hz should NOT appear near DC in the output.
    #[test]
    fn sine_offset_from_f0_not_at_dc() {
        let f0 = 1000.0f32;
        // Signal at f0 + 100 Hz (within the extracted band, but not at DC)
        let audio: Vec<i16> = (0..NMAX)
            .map(|n| {
                let t = n as f32 / 12_000.0;
                (10_000.0 * (2.0 * std::f32::consts::PI * (f0 + 100.0) * t).sin()) as i16
            })
            .collect();

        let out = downsample_simple(&audio, f0);

        let mut spectrum = out.clone();
        let mut planner = rustfft::FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(NFFT2);
        fft.process(&mut spectrum);

        // Energy near DC should NOT dominate (signal is 100 Hz away from f0)
        let energy_near_dc: f32 = spectrum[..=2].iter().map(|c| c.norm_sqr()).sum();
        let total_energy: f32 = spectrum.iter().map(|c| c.norm_sqr()).sum();

        // DC fraction should be small
        let frac = energy_near_dc / total_energy;
        assert!(
            frac < 0.1,
            "energy at DC fraction = {frac:.3} should be < 10% for off-frequency signal"
        );
    }

    /// Output must always be exactly NFFT2 = 3200 samples.
    #[test]
    fn output_length() {
        let audio = vec![0i16; NMAX];
        let out = downsample_simple(&audio, 1000.0);
        assert_eq!(out.len(), NFFT2);
    }

    /// Silence input should produce near-zero output.
    #[test]
    fn silence_gives_zero_output() {
        let audio = vec![0i16; NMAX];
        let out = downsample_simple(&audio, 1500.0);
        let max_abs = out.iter().map(|c| c.norm()).fold(0.0f32, f32::max);
        assert!(max_abs < 1e-10, "silent input produced non-zero output: {max_abs}");
    }
}
