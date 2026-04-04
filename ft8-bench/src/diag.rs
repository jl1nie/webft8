/// Diagnostic tracing for the decode pipeline.
use std::path::Path;

use ft8_core::{
    downsample::downsample,
    ldpc::bp::bp_decode,
    llr::{compute_llr, symbol_spectra, sync_quality},
    params::BP_MAX_ITER,
    sync::{coarse_sync, refine_candidate},
};

fn llr_stats(llr: &[f32]) -> (f32, f32, usize) {
    let min = llr.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = llr.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let pos = llr.iter().filter(|&&v| v > 0.0).count();
    (min, max, pos)
}

pub fn trace_pipeline(wav_path: &Path) -> Result<(), String> {
    let mut reader = hound::WavReader::open(wav_path)
        .map_err(|e| format!("open WAV: {e}"))?;
    let spec = reader.spec();
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read: {e}"))?;

    println!("=== trace: {} ===", wav_path.display());
    println!("  {samples_n} samples @ {sr} Hz", samples_n = samples.len(), sr = spec.sample_rate);

    // ── Coarse sync ─────────────────────────────────────────────────────────
    let cands = coarse_sync(&samples, 200.0, 2800.0, 0.0, None, 500);
    println!("  coarse_sync: {} raw candidates (no threshold)", cands.len());
    if cands.is_empty() {
        println!("  → no candidates at all");
        return Ok(());
    }
    // Print top 5
    for (i, c) in cands.iter().take(5).enumerate() {
        println!("  cand[{i:2}] freq={:7.1} Hz  dt={:+.3} s  score={:.4}", c.freq_hz, c.dt_sec, c.score);
    }

    // ── Try top 30 with full pipeline ────────────────────────────────────────
    let mut fft_cache: Option<Vec<_>> = None;
    let mut decoded_count = 0usize;
    for (ci, cand) in cands.iter().take(30).enumerate() {
        let (cd0, new_cache) = downsample(&samples, cand.freq_hz, fft_cache.as_deref());
        fft_cache = Some(new_cache);

        let refined = refine_candidate(&cd0, cand, 10);
        let i_start = ((refined.dt_sec + 0.5) * 200.0).round() as usize;
        let cs = symbol_spectra(&cd0, i_start);
        let nsync = sync_quality(&cs);
        if nsync <= 6 { continue; }

        let llr_set = compute_llr(&cs);
        let (lmin, lmax, lpos) = llr_stats(&llr_set.llra);

        let bp_a = bp_decode(&llr_set.llra, None, BP_MAX_ITER);
        if let Some(ref bp) = bp_a {
            let hex: String = bp.message77.iter().map(|b| format!("{b:01x}")).collect();
            let all_zero = bp.message77.iter().all(|&b| b == 0);
            println!(
                "  DECODED cand[{ci:2}] @{:.1}Hz sync_q={nsync:2} errs={} \
                 llra[min={lmin:.2} max={lmax:.2} +count={lpos}] \
                 msg={hex}{}",
                cand.freq_hz,
                bp.hard_errors,
                if all_zero { " ← ALL ZERO" } else { "" }
            );
            decoded_count += 1;
        } else {
            let bp_b = bp_decode(&llr_set.llrb, None, BP_MAX_ITER);
            let bp_c = bp_decode(&llr_set.llrc, None, BP_MAX_ITER);
            let bp_d = bp_decode(&llr_set.llrd, None, BP_MAX_ITER);
            if bp_b.is_some() || bp_c.is_some() || bp_d.is_some() {
                println!(
                    "  DECODED(bcd) cand[{ci:2}] @{:.1}Hz sync_q={nsync:2} \
                     llra[min={lmin:.2} max={lmax:.2} +count={lpos}]",
                    cand.freq_hz
                );
                decoded_count += 1;
            }
        }
    }
    println!("  → {decoded_count} decoded in top-30 candidates");
    Ok(())
}
