mod bpf;
mod ft4_sim;
mod real_data;
mod diag;
mod simulator;

use std::path::PathBuf;
use real_data::evaluate_real_data;
use mfsk_core::ft8::Ft8;
use mfsk_core::ft8::decode::{ApHint, DecodeDepth, DecodeResult, DecodeStrictness, EqMode, LlrEffort};
use mfsk_core::msg::decode_request::{DecodeRequest, SniperRequest};
use simulator::{make_busy_band_scenario, build_cq_messages};

/// The depth WebFT8 actually ships — `DecodeDepth::FULL`. See
/// `ft8-web/src/lib.rs`'s `SHIPPED_DEPTH` doc comment: the 2026-07-26
/// depth-matrix investigation tried `{Minimal, osd:true}` and found it
/// matched `Full` on a real recording and narrow-band sniper scenarios,
/// but was actively worse (both slower and lower recall) on a dense
/// 100-station scenario with real mutual interference — see
/// `run_depth_matrix_scenario`'s dense-station check in `run_speed_bench`.
/// `Full` never lost anywhere tested. Every scenario below except
/// `run_depth_matrix_scenario` itself (which deliberately sweeps all four
/// `{LlrEffort, osd}` combinations) uses this so the benchmark suite
/// reflects what actually ships.
const SHIPPED_DEPTH: DecodeDepth = DecodeDepth::FULL;

// ────────────────────────────────────────────────────────────────────────────
// Compatibility shims over the deleted `decode_frame*`/`decode_sniper*`
// function family (mfsk-core issue #191 consolidated them into
// `DecodeRequest`/`SniperRequest`). Kept as free functions with the old
// signatures so the many call sites below don't need touching one by one.
// ────────────────────────────────────────────────────────────────────────────

fn decode_frame(
    audio: &[i16], freq_min: f32, freq_max: f32, sync_min: f32,
    _freq_hint: Option<f32>, depth: DecodeDepth, max_cand: usize,
) -> Vec<DecodeResult> {
    DecodeRequest::<Ft8>::new(audio, freq_min, freq_max, sync_min, max_cand)
        .depth(depth)
        .decode()
        .results
}

#[allow(clippy::too_many_arguments)]
fn decode_frame_subtract(
    audio: &[i16], freq_min: f32, freq_max: f32, sync_min: f32,
    _freq_hint: Option<f32>, depth: DecodeDepth, max_cand: usize, strictness: DecodeStrictness,
) -> Vec<DecodeResult> {
    DecodeRequest::<Ft8>::new(audio, freq_min, freq_max, sync_min, max_cand)
        .depth(depth)
        .strictness(strictness)
        .staged()
        .decode()
        .results
}

#[allow(clippy::too_many_arguments)]
fn decode_frame_subtract_with_ap(
    audio: &[i16], freq_min: f32, freq_max: f32, sync_min: f32,
    _freq_hint: Option<f32>, depth: DecodeDepth, max_cand: usize, strictness: DecodeStrictness,
    ap_hint: Option<&ApHint>,
) -> Vec<DecodeResult> {
    let mut req = DecodeRequest::<Ft8>::new(audio, freq_min, freq_max, sync_min, max_cand)
        .depth(depth)
        .strictness(strictness);
    if let Some(ap) = ap_hint {
        req = req.ap_hint(ap);
    }
    req.staged().decode().results
}

fn decode_sniper(audio: &[i16], target_freq: f32, depth: DecodeDepth, max_cand: usize) -> Vec<DecodeResult> {
    SniperRequest::<Ft8>::new(audio, target_freq, max_cand)
        .depth(depth)
        .decode()
        .results
}

fn decode_sniper_eq(
    audio: &[i16], target_freq: f32, depth: DecodeDepth, max_cand: usize, eq_mode: EqMode,
) -> Vec<DecodeResult> {
    SniperRequest::<Ft8>::new(audio, target_freq, max_cand)
        .depth(depth)
        .eq_mode(eq_mode)
        .decode()
        .results
}

fn decode_sniper_ap(
    audio: &[i16], target_freq: f32, depth: DecodeDepth, max_cand: usize, eq_mode: EqMode,
    ap_hint: Option<&ApHint>,
) -> Vec<DecodeResult> {
    let mut req = SniperRequest::<Ft8>::new(audio, target_freq, max_cand)
        .depth(depth)
        .eq_mode(eq_mode);
    if let Some(ap) = ap_hint {
        req = req.ap_hint(ap);
    }
    req.decode().results
}

/// Replaces the deleted `decode_sniper_sic` (old ad-hoc 2-pass in-band SIC) —
/// per the benchmarked comparison in `run_bpf_subtract_scenario`'s SNR sweep,
/// narrow-band staged-checkpoint SIC + working `eq_mode` (mfsk-core commit
/// fe286cc) beats the old mechanism everywhere it was tried. This is exactly
/// what `ft8-web`'s `sniper_decode` ships.
fn decode_sniper_staged(
    audio: &[i16], target_freq: f32, depth: DecodeDepth, max_cand: usize, eq_mode: EqMode,
    ap_hint: Option<&ApHint>,
) -> Vec<DecodeResult> {
    let freq_min = (target_freq - 250.0).max(100.0);
    let freq_max = (target_freq + 250.0).min(5900.0);
    let mut req = DecodeRequest::<Ft8>::new(audio, freq_min, freq_max, 0.8, max_cand)
        .depth(depth)
        .eq_mode(eq_mode);
    if let Some(ap) = ap_hint {
        req = req.ap_hint(ap);
    }
    req.staged().decode().results
}

fn main() {
    let testdata = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");

    let wavs = [
        "191111_110130.wav",
        "191111_110200.wav",
    ];

    let mut total_decoded = 0usize;
    let mut any_found = false;

    for name in &wavs {
        let path = testdata.join(name);
        if !path.exists() {
            println!("SKIP {name} (not found — download from https://github.com/jl1nie/RustFT8/tree/main/data)");
            continue;
        }
        any_found = true;
        match evaluate_real_data(&path) {
            Ok(report) => {
                total_decoded += report.messages.len();
                report.print();
            }
            Err(e) => eprintln!("ERROR {name}: {e}"),
        }
    }

    if any_found {
        println!("Total decoded across all files: {total_decoded}");
    }

    // ── Synthetic interference scenario ─────────────────────────────────────
    run_interference_scenario();

    // ── Busy-band (ADC dynamic-range) scenario ───────────────────────────────
    run_busy_band_scenario();

    // ── Busy-band hard case (+20 dB crowd, −20 dB target) ────────────────────
    run_busy_band_hard_scenario();

    // ── Sniper story: no-BPF vs BPF-edge vs BPF-center × SIC/AP ──────
    run_sniper_story_scenario();

    // ── BPF filter scenarios (center / shoulder / edge) ─────────────────────────
    run_bpf_scenarios();

    // ── DecodeDepth {LlrEffort x osd} matrix on the sniper (staged+EQ+AP) path ──
    run_depth_matrix_scenario();

    // ── WSJT-X stress test WAV ───────────────────────────────────────────────
    run_wsjt_stress_test();

    // ── Speed benchmark (release build only meaningful) ───────────────────────
    run_speed_bench();

    // Diagnose missing signals in 110200
    let wav200 = testdata.join("191111_110200.wav");
    if wav200.exists() {
        println!();
        let _ = diag::trace_missing(&wav200);
    }

    // Diagnose OSD-only signals in 110130 (are they real or spurious?)
    let wav130 = testdata.join("191111_110130.wav");
    if wav130.exists() {
        println!();
        let _ = diag::trace_spurious(&wav130);
    }

    // ── Extreme limit sweep ─────────────────────────────────────────────────
    run_extreme_sweep();

    // ── FT4 SNR sweep (synthetic) ───────────────────────────────────────────
    run_ft4_snr_sweep();
}

fn run_ft4_snr_sweep() {
    use mfsk_core::ft4::Ft4;
    use mfsk_core::ft4::decode::ApHint;
    use mfsk_core::core::equalize::EqMode;
    use mfsk_core::msg::decode_request::{DecodeRequest, SniperRequest};
    use mfsk_core::{MessageCodec, MessageFields};

    println!("\n=== FT4 synthetic SNR sweep (20 seeds/SNR) ===");
    println!("  Columns: basic = decode_frame, AP = decode_sniper_ap(EQ=Adaptive, CQ+JA1ABC)");
    println!(
        "  {:>6}   {:>10}   {:>10}",
        "SNR", "basic/20", "AP/20"
    );

    let codec = mfsk_core::msg::Wsjt77Message::default();
    let msg77: [u8; 77] = {
        let bits = codec
            .pack(&MessageFields {
                call1: Some("CQ".into()),
                call2: Some("JA1ABC".into()),
                grid: Some("PM95".into()),
                ..Default::default()
            })
            .expect("pack CQ");
        let mut out = [0u8; 77];
        out.copy_from_slice(&bits);
        out
    };

    let ap = ApHint::new().with_call1("CQ").with_call2("JA1ABC");

    use rayon::prelude::*;
    for snr in [-4, -6, -8, -10, -12, -14, -16, -18, -20] {
        let (ok_basic, ok_ap) = (0..20u64)
            .into_par_iter()
            .map(|seed| {
                let cfg = ft4_sim::SimConfig {
                    signals: vec![ft4_sim::SimSignal {
                        message77: msg77,
                        freq_hz: 1000.0,
                        snr_db: snr as f32,
                        dt_sec: 0.0,
                    }],
                    noise_seed: Some(0xF70000 + seed),
                };
                let audio = ft4_sim::generate_slot(&cfg);
                let hit_basic = DecodeRequest::<Ft4>::new(&audio, 800.0, 1200.0, 1.2, 50)
                    .decode()
                    .results
                    .iter()
                    .any(|r| r.message77() == msg77);
                let hit_ap = SniperRequest::<Ft4>::new(&audio, 1000.0, 30)
                    .eq_mode(EqMode::Local)
                    .ap_hint(&ap)
                    .decode()
                    .results
                    .iter()
                    .any(|r| r.message77() == msg77);
                (hit_basic as usize, hit_ap as usize)
            })
            .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        println!("  {:>4} dB    {:>5}/20    {:>5}/20", snr, ok_basic, ok_ap);
    }
}

// ────────────────────────────────────────────────────────────────────────────

/// Fake JA callsigns (Q-code suffixes — impossible in real amateur allocations)
/// paired with JA-area grid locators for crowd CQ messages.
/// Sweep OSD thresholds to find the best Normal parameters.
fn crowd_calls_grids() -> Vec<(&'static str, &'static str)> {
    vec![
        ("JQ1QSO", "PM95"), ("JQ1QRM", "PM95"), ("JQ1QRN", "PM96"),
        ("JQ1QRP", "PM85"), ("JQ1QRT", "QM06"), ("JQ1QRV", "QM07"),
        ("JQ1QRZ", "PM74"), ("JQ1QSB", "PM84"), ("JQ1QSL", "PM86"),
        ("JQ1QSY", "QN01"), ("JQ1QTH", "QN02"), ("JQ1QRA", "PM75"),
        ("JQ1QRG", "PM94"), ("JQ1QRI", "QM05"), ("JQ1QRK", "PM83"),
    ]
}

/// Busy-band ADC dynamic-range scenario.
///
/// 12 strong crowd stations calling CQ with fake JA callsigns fill 200–2800 Hz.
/// A single weak 3Y0Z (Bouvet) target sits at 1000 Hz at −12 dB SNR.
///
/// Expected result:
///   - Full-band decode: target is NOT decoded (ADC range dominated by crowd)
///   - Sniper decode (target ±250 Hz): target IS decoded (crowd outside BPF)
fn run_busy_band_scenario() {
    use mfsk_core::ft8::message::{pack77_type1, unpack77};

    const TARGET_FREQ: f32 = 1000.0;
    const TARGET_SNR: f32 = -12.0;
    const INTERFERER_SNR: f32 = 5.0;

    let target_msg = pack77_type1("CQ", "3Y0Z", "JD34")
        .expect("failed to pack target message");

    let crowd = crowd_calls_grids();
    let interferer_msgs = build_cq_messages(&crowd);
    let num_crowd = interferer_msgs.len();

    println!("=== Busy-band: {num_crowd} crowd stations @ {INTERFERER_SNR:+.0} dB, target @ {TARGET_SNR:+.0} dB ===");
    println!("  target: {}", unpack77(&target_msg).unwrap_or_default());

    let config = make_busy_band_scenario(
        target_msg,
        TARGET_FREQ,
        TARGET_SNR,
        &interferer_msgs,
        INTERFERER_SNR,
        Some(777),
    );

    println!("  Crowd station frequencies (Hz):");
    for sig in config.signals.iter().skip(1) {
        print!("    {:6.1}", sig.freq_hz);
    }
    println!();

    let audio = simulator::generate_frame(&config);

    // Full-band decode (simulates WSJT-X)
    let results_full = decode_frame(
        &audio, 200.0, 2800.0, 1.0, None, SHIPPED_DEPTH, 200,
    );
    let target_full = results_full.iter().any(|r| r.message77 == target_msg);
    println!(
        "  [full-band  ] total decoded: {:2}  target @ {TARGET_FREQ:.0} Hz: {}",
        results_full.len(),
        if target_full { "DECODED" } else { "missed" }
    );
    for r in &results_full {
        if let Some(text) = unpack77(&r.message77) {
            println!("    {:+4.0} dB  {:7.1} Hz  {}", r.snr_db, r.freq_hz, text);
        }
    }

    // Sniper-mode decode (simulates hardware 500 Hz BPF removing the crowd)
    let results_sniper = decode_sniper(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20);
    let target_sniper = results_sniper.iter().any(|r| r.message77 == target_msg);
    println!(
        "  [sniper-mode] total decoded: {:2}  target @ {TARGET_FREQ:.0} Hz: {}",
        results_sniper.len(),
        if target_sniper { "DECODED" } else { "missed" }
    );

    // Write busy-band WAV for external WSJT-X verification
    let out_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("sim_busy_band.wav");
    if let Ok(()) = simulator::write_wav(&out_path, &audio) {
        println!("  WAV written: {}", out_path.display());
    }
    println!();
}

// ────────────────────────────────────────────────────────────────────────────

/// Hard busy-band scenario: +20 dB crowd, −20 dB target.
///
/// This is the extreme ADC saturation case.  The 40 dB gap means the target
/// sits 40 dB below the crowd — identical dynamic-range challenge to the
/// +40 dB two-station case, but spread across 12 stations so the ADC stitches
/// up all its headroom for the crowd.
///
/// Expected:
///   - Full-band (WSJT-X equivalent): target missed
///   - Sniper-mode (500 Hz BPF removes crowd): target decoded
fn run_busy_band_hard_scenario() {
    use mfsk_core::ft8::message::pack77_type1;

    const TARGET_FREQ: f32 = 1000.0;
    const TARGET_SNR: f32 = -14.0;  // 100% decode in BPF mode
    const INTERFERER_SNR: f32 = 40.0;  // 54 dB above target; hard-clips 16-bit ADC

    let target_msg = pack77_type1("CQ", "3Y0Z", "JD34")
        .expect("failed to pack target message");

    let crowd = crowd_calls_grids();
    let interferer_msgs = build_cq_messages(&crowd);
    let num_crowd = interferer_msgs.len();

    println!("=== Busy-band HARD: {num_crowd} crowd @ {INTERFERER_SNR:+.0} dB, target @ {TARGET_SNR:+.0} dB  (gap={:.0} dB) ===",
        INTERFERER_SNR - TARGET_SNR);

    let config = make_busy_band_scenario(
        target_msg,
        TARGET_FREQ,
        TARGET_SNR,
        &interferer_msgs,
        INTERFERER_SNR,
        Some(888),
    );

    println!("  Crowd station frequencies (Hz):");
    for sig in config.signals.iter().skip(1) {
        print!("    {:6.1}", sig.freq_hz);
    }
    println!();

    // ── Mixed audio with crowd-AGC quantisation ───────────────────────────────
    // The ADC gain is set for the +20 dB crowd.  The −16 dB target occupies
    // only the bottom few quantisation levels → buried in clipping/quantisation
    // noise from the crowd.  This is the real-world ADC dynamic-range problem.
    let mix_f32 = simulator::generate_frame_f32(&config);
    let audio_mixed = simulator::quantise_crowd_agc(&mix_f32, INTERFERER_SNR, num_crowd);

    let results_full = decode_frame(
        &audio_mixed, 200.0, 2800.0, 1.0, None, SHIPPED_DEPTH, 200,
    );
    let target_full = results_full.iter().any(|r| r.message77 == target_msg);
    println!(
        "  [no-BPF: full-band ] total decoded: {:2}  target @ {TARGET_FREQ:.0} Hz: {}",
        results_full.len(),
        if target_full { "DECODED" } else { "missed" }
    );

    // Narrow-band search on mixed ADC audio (crowd distortion still present)
    let results_sniper_mixed = decode_sniper(&audio_mixed, TARGET_FREQ, SHIPPED_DEPTH, 20);
    let target_mixed = results_sniper_mixed.iter().any(|r| r.message77 == target_msg);
    println!(
        "  [no-BPF: sniper sw ] total decoded: {:2}  target @ {TARGET_FREQ:.0} Hz: {}",
        results_sniper_mixed.len(),
        if target_mixed { "DECODED" } else { "missed" }
    );

    // ── i16 quantisation headroom probe (single seed) ────────────────────────
    // Same f32 mix, but scaled so the strongest sample fits at ~88% of i16
    // range (no AGC clipping). The crowd is no longer hard-clipped, but the
    // weak target still has only a few LSBs of dynamic range to work with.
    let audio_clean_quant = simulator::generate_frame(&config);
    let results_clean_full = decode_frame(
        &audio_clean_quant, 200.0, 2800.0, 1.0, None, SHIPPED_DEPTH, 200,
    );
    let target_clean_full = results_clean_full.iter().any(|r| r.message77 == target_msg);
    let results_clean_sniper = decode_sniper(
        &audio_clean_quant, TARGET_FREQ, SHIPPED_DEPTH, 20,
    );
    let target_clean_sniper = results_clean_sniper.iter().any(|r| r.message77 == target_msg);
    println!(
        "  [i16 clean: full   ] total decoded: {:2}  target @ {TARGET_FREQ:.0} Hz: {}",
        results_clean_full.len(),
        if target_clean_full { "DECODED" } else { "missed" }
    );
    println!(
        "  [i16 clean: sniper ] total decoded: {:2}  target @ {TARGET_FREQ:.0} Hz: {}",
        results_clean_sniper.len(),
        if target_clean_sniper { "DECODED" } else { "missed" }
    );

    // ── i16 quantisation sweep: 30 seeds, AGC vs clean side-by-side ─────────
    // Statistical comparison of AGC clipping vs clean i16 quantisation on the
    // *same* set of noise realisations. If clean wins clearly, AGC clipping
    // distortion is the bottleneck (→ physical BPF helps). If AGC wins or
    // they tie, i16 LSB quantisation itself is the limit (→ a future
    // f32-native ft8-core would unlock more).
    const SWEEP_SEEDS: u64 = 30;
    use rayon::prelude::*;
    let (agc_full_ok, agc_sniper_ok, clean_full_ok, clean_sniper_ok) = (0..SWEEP_SEEDS)
        .into_par_iter()
        .map(|seed| {
            let cfg = make_busy_band_scenario(
                target_msg, TARGET_FREQ, TARGET_SNR,
                &interferer_msgs, INTERFERER_SNR,
                Some(seed),
            );
            let f32_mix = simulator::generate_frame_f32(&cfg);
            let audio_agc   = simulator::quantise_crowd_agc(&f32_mix, INTERFERER_SNR, num_crowd);
            let audio_clean = simulator::generate_frame(&cfg);

            let hit1 = decode_frame(&audio_agc,   200.0, 2800.0, 1.0, None, SHIPPED_DEPTH, 200)
                .iter().any(|r| r.message77 == target_msg);
            let hit2 = decode_sniper(&audio_agc,   TARGET_FREQ, SHIPPED_DEPTH, 20)
                .iter().any(|r| r.message77 == target_msg);
            let hit3 = decode_frame(&audio_clean, 200.0, 2800.0, 1.0, None, SHIPPED_DEPTH, 200)
                .iter().any(|r| r.message77 == target_msg);
            let hit4 = decode_sniper(&audio_clean, TARGET_FREQ, SHIPPED_DEPTH, 20)
                .iter().any(|r| r.message77 == target_msg);

            (hit1 as usize, hit2 as usize, hit3 as usize, hit4 as usize)
        })
        .reduce(|| (0, 0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2, a.3 + b.3));
    println!("  ── i16 quantisation sweep ({} seeds) ──", SWEEP_SEEDS);
    println!("  [AGC   full-band ] target hits: {:2}/{SWEEP_SEEDS}  ({:>3.0}%)",
        agc_full_ok, 100.0 * agc_full_ok as f32 / SWEEP_SEEDS as f32);
    println!("  [AGC   sniper sw ] target hits: {:2}/{SWEEP_SEEDS}  ({:>3.0}%)",
        agc_sniper_ok, 100.0 * agc_sniper_ok as f32 / SWEEP_SEEDS as f32);
    println!("  [clean full-band ] target hits: {:2}/{SWEEP_SEEDS}  ({:>3.0}%)",
        clean_full_ok, 100.0 * clean_full_ok as f32 / SWEEP_SEEDS as f32);
    println!("  [clean sniper sw ] target hits: {:2}/{SWEEP_SEEDS}  ({:>3.0}%)",
        clean_sniper_ok, 100.0 * clean_sniper_ok as f32 / SWEEP_SEEDS as f32);

    // Write crowd-AGC mixed WAV for WSJT-X external verification
    let out_mixed = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata").join("sim_busy_band_hard_mixed.wav");
    if simulator::write_wav(&out_mixed, &audio_mixed).is_ok() {
        println!("  WAV (crowd-AGC mixed) written: {}", out_mixed.display());
    }
    // Write i16-clean WAV (no AGC clipping) so WSJT-X can be tested on it.
    // Compare to the AGC version: any extra decodes here come from removing
    // clipping distortion, not from i16 quantisation headroom.
    let out_clean = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata").join("sim_busy_band_hard_clean.wav");
    if simulator::write_wav(&out_clean, &audio_clean_quant).is_ok() {
        println!("  WAV (i16 clean, no clip) written: {}", out_clean.display());
    }
    // Write BPF WAV (seed=0) as the cleanest target-only reference
    {
        let config_bpf0 = simulator::SimConfig {
            signals: vec![simulator::SimSignal {
                message77: target_msg, freq_hz: TARGET_FREQ,
                snr_db: TARGET_SNR, dt_sec: 0.0,
            }],
            noise_seed: Some(0),
        };
        let audio_bpf0 = simulator::generate_frame(&config_bpf0);
        let out_bpf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata").join("sim_busy_band_hard_bpf.wav");
        if simulator::write_wav(&out_bpf, &audio_bpf0).is_ok() {
            println!("  WAV (BPF, seed=0) written: {}", out_bpf.display());
        }
    }
    println!();
}

// ────────────────────────────────────────────────────────────────────────────

/// The Sniper Story: hardware BPF position vs SIC/AP effectiveness.
///
/// This is the core argument for sniper mode. Without a hardware BPF the ADC
/// is dominated by the strong crowd (40 dB above target) and SIC/AP cannot
/// recover useful LLRs from the saturated signal. As the BPF moves the target
/// from the passband edge toward the centre, distortion falls, effective SNR
/// rises, and SIC/AP gain becomes measurable.
///
/// Three configurations, same 15-station crowd at +20 dB, 20 noise seeds:
///   1. **No-BPF**:     full i16 mix  -> ADC saturated, SIC/AP helpless
///   2. **BPF-edge**:        500 Hz Butterworth BPF, target at -3 dB edge
///   3. **BPF-center**:      500 Hz Butterworth BPF, target at 0 dB centre
///   4. **Elliptic-edge**:   500 Hz Elliptic BPF,    target at -3 dB edge
///   5. **Elliptic-center**: 500 Hz Elliptic BPF,    target at 0 dB centre
fn run_sniper_story_scenario() {
    use mfsk_core::ft8::decode::ApHint;
    use mfsk_core::ft8::decode::EqMode;
    use bpf::{ButterworthBpf, EllipticBpf};

    const TARGET_FREQ: f32 = 1000.0;
    const CROWD_SNR:   f32 = 40.0;
    const N_POLES: usize = 4;
    const FS: f64 = 12_000.0;
    const N_SEEDS: u64 = 20;

    let target_msg = pack77_test_type1("CQ", "3Y0Z", "JD34");

    let crowd = crowd_calls_grids();
    let crowd_msgs = build_cq_messages(&crowd);
    let num_crowd = crowd_msgs.len();

    let ap = ApHint::new().with_call2("3Y0Z");

    println!("=== Sniper Story: Butterworth vs Elliptic (4-pole, 500 Hz BW) ===");
    println!("  crowd: {num_crowd} stations @ {CROWD_SNR:+.0} dB (ADC-saturating)");
    println!("  AP hint: call2 = '3Y0Z' | EQ+AP applied to all BPF columns");
    println!();
    println!("  {:>6}  {:>14}  {:>16}  {:>16}  {:>16}  {:>16}",
        "SNR", "no-BPF",
        "BW-edge+EQ+AP", "BW-center+EQ+AP",
        "EL-edge+EQ+AP", "EL-center+EQ+AP");

    // Print Elliptic filter response for reference
    {
        let el = EllipticBpf::design(N_POLES, 1000.0, 1500.0, FS);
        println!("  Elliptic edge BPF 1000–1500 Hz response:");
        for &f in &[800.0, 900.0, 1000.0, 1100.0, 1200.0, 1300.0, 1500.0, 1800.0] {
            println!("    {:>6.0} Hz: {:>6.1} dB", f, el.response_db(f, FS));
        }
        println!();
    }

    use rayon::prelude::*;

    for snr in [-10i32, -12, -14, -16, -18, -20, -22] {
        let target_snr = snr as f32;

        let (ok_noisy, ok_bw_edge, ok_bw_ctr, ok_el_edge, ok_el_ctr) = (0..N_SEEDS)
            .into_par_iter()
            .map(|seed| {
                // --- 1. No-BPF ---------------------------------------------------
                let cfg_full = simulator::make_busy_band_scenario(
                    target_msg, TARGET_FREQ, target_snr, &crowd_msgs, CROWD_SNR, Some(seed),
                );
                let mix_full = simulator::generate_frame_f32(&cfg_full);
                let audio_noisy = simulator::quantise_crowd_agc(&mix_full, CROWD_SNR, num_crowd);
                let hit_noisy = decode_sniper(&audio_noisy, TARGET_FREQ, SHIPPED_DEPTH, 20)
                    .iter().any(|r| r.message77 == target_msg);

                // Target-only mix (hardware BPF removes crowd before ADC)
                let cfg_bpf = simulator::SimConfig {
                    signals: vec![simulator::SimSignal {
                        message77: target_msg,
                        freq_hz: TARGET_FREQ,
                        snr_db: target_snr,
                        dt_sec: 0.0,
                    }],
                    noise_seed: Some(seed),
                };
                let mix_bpf = simulator::generate_frame_f32(&cfg_bpf);

                // helper: filter → normalise → i16 → decode
                macro_rules! try_bpf {
                    ($filtered:expr) => {{
                        let filt = $filtered;
                        let peak = filt.iter().map(|s: &f32| s.abs()).fold(0.0_f32, f32::max);
                        let scale = if peak > 1e-6 { 29_000.0 / peak } else { 1.0 };
                        let audio: Vec<i16> = filt.iter()
                            .map(|&s| (s * scale).clamp(-32_768.0, 32_767.0) as i16).collect();
                        decode_sniper_staged(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20,
                                EqMode::Local, Some(&ap))
                            .iter().any(|r| r.message77 == target_msg)
                    }};
                }

                // --- 2. Butterworth-edge -----------------------------------------
                let mut bpf = ButterworthBpf::design(N_POLES, 1000.0, 1500.0, FS);
                let hit_bw_edge = try_bpf!(bpf.filter(&mix_bpf));

                // --- 3. Butterworth-center ----------------------------------------
                let mut bpf = ButterworthBpf::design(N_POLES, 750.0, 1250.0, FS);
                let hit_bw_ctr = try_bpf!(bpf.filter(&mix_bpf));

                // --- 4. Elliptic-edge ---------------------------------------------
                let mut bpf = EllipticBpf::design(N_POLES, 1000.0, 1500.0, FS);
                let hit_el_edge = try_bpf!(bpf.filter(&mix_bpf));

                // --- 5. Elliptic-center -------------------------------------------
                let mut bpf = EllipticBpf::design(N_POLES, 750.0, 1250.0, FS);
                let hit_el_ctr = try_bpf!(bpf.filter(&mix_bpf));

                (
                    hit_noisy as usize,
                    hit_bw_edge as usize,
                    hit_bw_ctr as usize,
                    hit_el_edge as usize,
                    hit_el_ctr as usize,
                )
            })
            .reduce(
                || (0, 0, 0, 0, 0),
                |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2, a.3 + b.3, a.4 + b.4),
            );

        let pct = |ok: usize| 100.0 * ok as f64 / N_SEEDS as f64;
        println!("  {:+4} dB  {:>4}/{N_SEEDS}({:>3.0}%)  {:>4}/{N_SEEDS}({:>3.0}%)  {:>4}/{N_SEEDS}({:>3.0}%)  {:>4}/{N_SEEDS}({:>3.0}%)  {:>4}/{N_SEEDS}({:>3.0}%)",
            snr,
            ok_noisy,   pct(ok_noisy),
            ok_bw_edge, pct(ok_bw_edge),
            ok_bw_ctr,  pct(ok_bw_ctr),
            ok_el_edge, pct(ok_el_edge),
            ok_el_ctr,  pct(ok_el_ctr),
        );
    }
    println!();
}

fn pack77_test_type1(cq: &str, call: &str, grid: &str) -> [u8; 77] {
    use mfsk_core::ft8::message::pack77_type1;
    pack77_type1(cq, call, grid).expect("failed to pack message")
}
// ────────────────────────────────────────────────────────────────────────────

/// BPF filter scenarios: demonstrate the effect of a 500 Hz hardware BPF on
/// signal placement (center, shoulder, edge).
///
/// For each sub-scenario, the same busy-band mix is generated but the hardware
/// BPF is centred so the target falls at a different position within the
/// passband.  After BPF filtering, the audio is re-quantised to 16 bits and
/// decoded.
///
/// * **Center:**   target at BPF center → minimal distortion, easy decode.
/// * **Shoulder:** target 200 Hz off-centre → moderate attenuation.
/// * **Edge:**     target at the −3 dB point → significant amplitude loss +
///   phase distortion.  Without an equalizer, decode may fail at low SNR.
fn run_bpf_scenarios() {
    use mfsk_core::ft8::message::pack77_type1;
    use bpf::ButterworthBpf;
    use rayon::prelude::*;

    const TARGET_FREQ: f32 = 1000.0;
    const TARGET_SNR: f32 = -18.0;
    const N_POLES: usize = 4; // 8th-order BPF (typical crystal CW filter)
    const BPF_BW: f64 = 500.0;
    const FS: f64 = 12_000.0;
    const N_SEEDS: u64 = 20;

    let target_msg = pack77_type1("CQ", "3Y0Z", "JD34")
        .expect("failed to pack target message");

    println!("=== BPF edge-effect scenarios: {N_POLES}-pole Butterworth ({BPF_BW:.0} Hz BW), target @ {TARGET_SNR:+.0} dB ===");
    println!("  (target + AWGN only — isolates filter distortion from crowd interference)");

    // Print reference filter response
    {
        let bpf = ButterworthBpf::design(N_POLES, 750.0, 1250.0, FS);
        println!("  Filter response (example: centre=1000 Hz):");
        for &f in &[750.0, 800.0, 900.0, 1000.0, 1100.0, 1200.0, 1250.0, 1300.0, 1500.0] {
            println!("    {:7.0} Hz: {:+6.1} dB", f, bpf.response_db(f, FS));
        }
    }

    // Sub-scenarios: (label, bpf_center_offset_from_target)
    // BPF passband = [bpf_center - 250, bpf_center + 250]
    // Offset 0: target at BPF centre; 200: shoulder; 250: at −3 dB edge
    let cases: [(&str, f64); 3] = [
        ("center",     0.0),   // target at BPF centre → ~0 dB atten
        ("shoulder", 200.0),   // target 200 Hz from centre → moderate atten
        ("edge",     250.0),   // target at −3 dB passband edge
    ];

    for &(label, offset) in &cases {
        let bpf_center = TARGET_FREQ as f64 + offset;
        let bpf_lo = bpf_center - BPF_BW / 2.0;
        let bpf_hi = bpf_center + BPF_BW / 2.0;

        let target_atten = {
            let bpf = ButterworthBpf::design(N_POLES, bpf_lo, bpf_hi, FS);
            bpf.response_db(TARGET_FREQ as f64, FS)
        };

        // Sweep seeds — compare EQ OFF vs EQ ON
        let (ok_off, ok_on) = (0..N_SEEDS)
            .into_par_iter()
            .map(|seed| {
                let config = simulator::SimConfig {
                    signals: vec![simulator::SimSignal {
                        message77: target_msg,
                        freq_hz: TARGET_FREQ,
                        snr_db: TARGET_SNR,
                        dt_sec: 0.0,
                    }],
                    noise_seed: Some(seed),
                };
                let mix = simulator::generate_frame_f32(&config);

                let mut bpf = ButterworthBpf::design(N_POLES, bpf_lo, bpf_hi, FS);
                let filtered = bpf.filter(&mix);

                let peak = filtered.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
                let scale = if peak > 1e-6 { 29_000.0 / peak } else { 1.0 };
                let audio: Vec<i16> = filtered
                    .iter()
                    .map(|&s| (s * scale).clamp(-32_768.0, 32_767.0) as i16)
                    .collect();

                let r_off = decode_sniper_eq(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Off);
                let hit_off = r_off.iter().any(|r| r.message77 == target_msg);

                let r_on = decode_sniper_eq(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local);
                let hit_on = r_on.iter().any(|r| r.message77 == target_msg);

                (hit_off as usize, hit_on as usize)
            })
            .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));

        println!(
            "  [{label:8}] BPF {bpf_lo:.0}–{bpf_hi:.0} Hz  atten={target_atten:+.1} dB  \
             EQ OFF: {ok_off}/{N_SEEDS} ({:.0}%)  EQ ON: {ok_on}/{N_SEEDS} ({:.0}%)",
            100.0 * ok_off as f64 / N_SEEDS as f64,
            100.0 * ok_on as f64 / N_SEEDS as f64,
        );

        // Write WAV (seed=0)
        {
            let config = simulator::SimConfig {
                signals: vec![simulator::SimSignal {
                    message77: target_msg,
                    freq_hz: TARGET_FREQ,
                    snr_db: TARGET_SNR,
                    dt_sec: 0.0,
                }],
                noise_seed: Some(0),
            };
            let mix = simulator::generate_frame_f32(&config);
            let mut bpf = ButterworthBpf::design(N_POLES, bpf_lo, bpf_hi, FS);
            let filtered = bpf.filter(&mix);
            let peak = filtered.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
            let scale = if peak > 1e-6 { 29_000.0 / peak } else { 1.0 };
            let audio: Vec<i16> = filtered
                .iter()
                .map(|&s| (s * scale).clamp(-32_768.0, 32_767.0) as i16)
                .collect();
            let wav_name = format!("sim_bpf_{label}.wav");
            let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("testdata")
                .join(&wav_name);
            let _ = simulator::write_wav(&out, &audio);
        }
    }

    // Baseline (no BPF) for reference
    {
        let ok_ref = (0..N_SEEDS)
            .into_par_iter()
            .filter(|&seed| {
                let config = simulator::SimConfig {
                    signals: vec![simulator::SimSignal {
                        message77: target_msg,
                        freq_hz: TARGET_FREQ,
                        snr_db: TARGET_SNR,
                        dt_sec: 0.0,
                    }],
                    noise_seed: Some(seed),
                };
                let audio = simulator::generate_frame(&config);
                let results = decode_sniper(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20);
                results.iter().any(|r| r.message77 == target_msg)
            })
            .count();
        println!(
            "  [no-BPF  ] reference (target+noise only)  {ok_ref}/{N_SEEDS} decoded ({:.0}%)",
            100.0 * ok_ref as f64 / N_SEEDS as f64
        );
    }
    println!();

    // ── BPF + in-band crowd + signal subtraction ─────────────────────────────
    // Even with the 500 Hz hardware BPF, a few crowd stations may fall inside
    // the passband.  Signal subtraction decodes and removes the strong crowd
    // first, then finds the weak target underneath.
    run_bpf_subtract_scenario();
}

/// BPF with in-band crowd stations: signal subtraction recovers the target.
///
/// 3 strong crowd stations within the 500 Hz passband plus a weak target.
/// Single-pass sniper: crowd masks the target.
/// Subtract-pass: crowd is decoded & subtracted → target emerges.
fn run_bpf_subtract_scenario() {
    use mfsk_core::ft8::message::{pack77_type1, unpack77};
    use bpf::ButterworthBpf;

    const TARGET_FREQ: f32 = 1000.0;
    const TARGET_SNR: f32 = -14.0;
    const N_POLES: usize = 4;
    const BPF_LO: f64 = 750.0;
    const BPF_HI: f64 = 1250.0;
    const FS: f64 = 12_000.0;

    let target_msg = pack77_type1("CQ", "3Y0Z", "JD34")
        .expect("failed to pack target message");

    // 4 crowd stations within BPF passband, some very close to the target.
    // The close stations (±50 Hz) cause spectral leakage into the target's
    // LLR computation, masking it in single-pass.  After subtraction of the
    // decoded crowd, the target emerges cleanly.
    let in_band_crowd: Vec<(f32, [u8; 77])> = vec![
        ( 850.0, pack77_type1("CQ", "JQ1QSO", "PM95").unwrap()),
        ( 950.0, pack77_type1("CQ", "JQ1QRM", "PM95").unwrap()),  // 50 Hz below target
        (1050.0, pack77_type1("CQ", "JQ1QRN", "PM96").unwrap()),  // 50 Hz above target
        (1150.0, pack77_type1("CQ", "JQ1QRP", "PM85").unwrap()),
    ];
    let crowd_snr: f32 = 8.0; // 22 dB above target

    println!("=== BPF + in-band crowd + signal subtraction ===");
    println!("  BPF {BPF_LO:.0}–{BPF_HI:.0} Hz, target @ {TARGET_SNR:+.0} dB, {} crowd @ {crowd_snr:+.0} dB inside passband",
        in_band_crowd.len());

    let mut signals = vec![simulator::SimSignal {
        message77: target_msg,
        freq_hz: TARGET_FREQ,
        snr_db: TARGET_SNR,
        dt_sec: 0.0,
    }];
    for &(freq, ref msg) in &in_band_crowd {
        signals.push(simulator::SimSignal {
            message77: *msg,
            freq_hz: freq,
            snr_db: crowd_snr,
            dt_sec: 0.0,
        });
    }

    let config = simulator::SimConfig {
        signals,
        noise_seed: Some(1234),
    };

    // Generate and apply BPF
    let mix = simulator::generate_frame_f32(&config);
    let mut bpf = ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
    let filtered = bpf.filter(&mix);

    // Quantise
    let peak = filtered.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    let scale = if peak > 1e-6 { 29_000.0 / peak } else { 1.0 };
    let audio: Vec<i16> = filtered
        .iter()
        .map(|&s| (s * scale).clamp(-32_768.0, 32_767.0) as i16)
        .collect();

    // Single-pass sniper
    let results_single = decode_sniper(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20);
    let target_single = results_single.iter().any(|r| r.message77 == target_msg);
    println!(
        "  [single-pass] decoded: {:2}  target: {}",
        results_single.len(),
        if target_single { "DECODED" } else { "missed" }
    );
    for r in &results_single {
        if let Some(text) = unpack77(&r.message77) {
            println!("    {:+5.1} dB  {:7.1} Hz  {}", r.snr_db, r.freq_hz, text);
        }
    }

    // Subtract-pass (multi-pass decode with signal subtraction)
    use mfsk_core::ft8::decode::DecodeStrictness;
    let results_sub = decode_frame_subtract(
        &audio, BPF_LO as f32, BPF_HI as f32, 0.8, None, SHIPPED_DEPTH, 20, DecodeStrictness::Normal,
    );
    let target_sub = results_sub.iter().any(|r| r.message77 == target_msg);
    println!(
        "  [subtract   ] decoded: {:2}  target: {}",
        results_sub.len(),
        if target_sub { "DECODED" } else { "missed" }
    );
    for r in &results_sub {
        if let Some(text) = unpack77(&r.message77) {
            let tag = if r.message77 == target_msg { " ★" } else { "" };
            println!("    {:+5.1} dB  {:7.1} Hz  pass={}  {}{tag}", r.snr_db, r.freq_hz, r.pass, text);
        }
    }

    // Sniper-SIC (in-band successive interference cancellation)
    let results_sic = decode_sniper_staged(
        &audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, None,
    );
    let target_sic = results_sic.iter().any(|r| r.message77 == target_msg);
    println!(
        "  [sniper-SIC ] decoded: {:2}  target: {}",
        results_sic.len(),
        if target_sic { "DECODED ★" } else { "missed" }
    );
    for r in &results_sic {
        if let Some(text) = unpack77(&r.message77) {
            let tag = if r.message77 == target_msg { " ★" } else { "" };
            println!("    {:+5.1} dB  {:7.1} Hz  pass={}  {}{tag}", r.snr_db, r.freq_hz, r.pass, text);
        }
    }

    // Write WAV (seed=1234 example)
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("sim_bpf_subtract.wav");
    if simulator::write_wav(&out, &audio).is_ok() {
        println!("  WAV: {}", out.display());
    }

    // ── Statistical sweep: 20 seeds × target SNR ────────────────────────────
    // Same BPF + 4 in-band crowd setup, vary target SNR and noise seed.
    // AP = target callsign known (call2 = "3Y0Z"), matching real GUI behaviour
    // where the user has locked the DX station before calling.
    use mfsk_core::ft8::decode::ApHint;
    let ap = ApHint::new().with_call2("3Y0Z");
    const N_SEEDS: u64 = 20;
    println!("  SNR sweep ({N_SEEDS} seeds each, AP = call2 '3Y0Z' known):");
    println!("  {:>6}  {:>12}  {:>10}  {:>16}  {:>16}  {:>16}  {:>16}",
        "SNR", "single-pass", "EQ-only", "subtract(staged)", "staged+AP(noEQ)", "sniper(staged+EQ)", "staged+EQ+AP");
    use rayon::prelude::*;
    for snr in [-10, -12, -14, -16, -18, -20] {
        let (ok_single, ok_eq, ok_sub, ok_staged_ap, ok_sic, ok_sic_ap) = (0..N_SEEDS)
            .into_par_iter()
            .map(|seed| {
                let mut sigs = vec![simulator::SimSignal {
                    message77: target_msg,
                    freq_hz: TARGET_FREQ,
                    snr_db: snr as f32,
                    dt_sec: 0.0,
                }];
                for &(freq, ref msg) in &in_band_crowd {
                    sigs.push(simulator::SimSignal {
                        message77: *msg,
                        freq_hz: freq,
                        snr_db: crowd_snr,
                        dt_sec: 0.0,
                    });
                }
                let cfg = simulator::SimConfig { signals: sigs, noise_seed: Some(seed) };
                let mix = simulator::generate_frame_f32(&cfg);
                let mut bpf_s = ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
                let filt = bpf_s.filter(&mix);
                let peak = filt.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
                let scale = if peak > 1e-6 { 29_000.0 / peak } else { 1.0 };
                let au: Vec<i16> = filt.iter()
                    .map(|&s| (s * scale).clamp(-32_768.0, 32_767.0) as i16)
                    .collect();

                // A: plain SniperRequest, no SIC, no EQ (old shipped default)
                let hit_single = decode_sniper(&au, TARGET_FREQ, SHIPPED_DEPTH, 20)
                    .iter().any(|r| r.message77 == target_msg);
                // A': plain SniperRequest + EQ, no SIC at all
                let hit_eq = decode_sniper_eq(&au, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local)
                    .iter().any(|r| r.message77 == target_msg);
                // B: staged-checkpoint SIC on the narrow BPF band, no EQ, no AP
                //    (decode_frame_subtract delegates to staged by default as of 756d81f7)
                let hit_sub = decode_frame_subtract(&au, BPF_LO as f32, BPF_HI as f32, 0.8, None,
                        SHIPPED_DEPTH, 20, DecodeStrictness::Normal)
                    .iter().any(|r| r.message77 == target_msg);
                // B': same staged-checkpoint SIC, narrow band, + AP hint, no EQ
                let hit_staged_ap = decode_frame_subtract_with_ap(&au, BPF_LO as f32, BPF_HI as f32, 0.8, None,
                        SHIPPED_DEPTH, 20, DecodeStrictness::Normal, Some(&ap))
                    .iter().any(|r| r.message77 == target_msg);
                // C: what ft8-web's sniper_decode actually ships — narrow-band
                //    staged-checkpoint SIC *with* EQ (mfsk-core commit fe286cc
                //    fixed .staged() silently dropping eq_mode), no AP. Replaces
                //    the old ad-hoc decode_sniper_sic (deleted upstream, and
                //    benchmarked worse in both this scenario and the BPF-edge
                //    one — see docs/bench.md).
                let hit_sic = decode_sniper_staged(&au, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, None)
                    .iter().any(|r| r.message77 == target_msg);
                // C': same, + AP hint
                let hit_sic_ap = decode_sniper_staged(&au, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, Some(&ap))
                    .iter().any(|r| r.message77 == target_msg);

                (hit_single as usize, hit_eq as usize, hit_sub as usize, hit_staged_ap as usize, hit_sic as usize, hit_sic_ap as usize)
            })
            .reduce(|| (0, 0, 0, 0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2, a.3 + b.3, a.4 + b.4, a.5 + b.5));
        println!("  {:+4} dB  {:>4}/{N_SEEDS}({:>3.0}%)  {:>4}/{N_SEEDS}({:>3.0}%)  {:>4}/{N_SEEDS}({:>3.0}%)  {:>4}/{N_SEEDS}({:>3.0}%)  {:>4}/{N_SEEDS}({:>3.0}%)  {:>4}/{N_SEEDS}({:>3.0}%)",
            snr,
            ok_single,     100.0 * ok_single     as f64 / N_SEEDS as f64,
            ok_eq,         100.0 * ok_eq         as f64 / N_SEEDS as f64,
            ok_sub,        100.0 * ok_sub        as f64 / N_SEEDS as f64,
            ok_staged_ap,  100.0 * ok_staged_ap  as f64 / N_SEEDS as f64,
            ok_sic,        100.0 * ok_sic        as f64 / N_SEEDS as f64,
            ok_sic_ap,     100.0 * ok_sic_ap     as f64 / N_SEEDS as f64,
        );
    }
    println!();
}

// ────────────────────────────────────────────────────────────────────────────

/// Depth matrix: sweep LlrEffort x osd (now independent DecodeDepth fields,
/// mfsk-core issue #188) against the sniper (staged+EQ+AP) narrow-band path,
/// on the same two crowd configurations that drove the `fe286cc` eq_mode
/// fix (see docs/bench.md Scenario 5) — crowd at the BPF center (4
/// interferers) and crowd at the BPF edge (fewer interferers, target itself
/// distorted). No named DecodeDepth const covers {Minimal, osd:true}; this
/// checks whether it's a free win here the way it was on the real recording
/// (see `examples/depth_matrix.rs`).
fn run_depth_matrix_scenario() {
    use bpf::ButterworthBpf;
    use mfsk_core::ft8::message::pack77_type1;
    use rayon::prelude::*;
    use std::time::Instant;

    const FS: f64 = 12_000.0;
    const N_POLES: usize = 4;
    const TARGET_FREQ: f32 = 1000.0;
    const N_SEEDS: u64 = 20;

    let target_msg = pack77_type1("CQ", "3Y0Z", "JD34").unwrap();
    let ap = ApHint::new().with_call2("3Y0Z");

    let depths = [
        ("Minimal,noOSD", DecodeDepth { llr_effort: LlrEffort::Minimal, osd: false }),
        ("Minimal,OSD  ", DecodeDepth { llr_effort: LlrEffort::Minimal, osd: true }),
        ("Full,noOSD   ", DecodeDepth { llr_effort: LlrEffort::Full, osd: false }),
        ("Full,OSD     ", DecodeDepth { llr_effort: LlrEffort::Full, osd: true }),
    ];

    println!("\n=== DEPTH MATRIX: sniper (staged+EQ+AP), narrow-band ===");

    struct Case {
        label: &'static str,
        bpf_lo: f64,
        bpf_hi: f64,
        crowd: Vec<(f32, [u8; 77])>,
        crowd_snr: f32,
        snrs: [i32; 2],
    }
    let cases = vec![
        Case {
            label: "center-crowd (BPF 750-1250 Hz, 4 in-band interferers @ +8dB)",
            bpf_lo: 750.0,
            bpf_hi: 1250.0,
            crowd: vec![
                (850.0, pack77_type1("CQ", "JQ1QSO", "PM95").unwrap()),
                (950.0, pack77_type1("CQ", "JQ1QRM", "PM95").unwrap()),
                (1050.0, pack77_type1("CQ", "JQ1QRN", "PM96").unwrap()),
                (1150.0, pack77_type1("CQ", "JQ1QRP", "PM85").unwrap()),
            ],
            crowd_snr: 8.0,
            snrs: [-20, -22],
        },
        Case {
            label: "BPF-edge (BPF 1000-1500 Hz, target at -3dB edge, 2 in-band interferers @ +8dB)",
            bpf_lo: 1000.0,
            bpf_hi: 1500.0,
            crowd: vec![
                (1050.0, pack77_type1("CQ", "JQ1QRN", "PM96").unwrap()),
                (1150.0, pack77_type1("CQ", "JQ1QRP", "PM85").unwrap()),
            ],
            crowd_snr: 8.0,
            snrs: [-20, -22],
        },
    ];

    for case in &cases {
        println!("\n-- {} --", case.label);
        for snr in case.snrs {
            print!("  SNR {snr:+3} dB:");
            for (name, depth) in &depths {
                let (ok, ms_sum) = (0..N_SEEDS)
                    .into_par_iter()
                    .map(|seed| {
                        let mut sigs = vec![simulator::SimSignal {
                            message77: target_msg,
                            freq_hz: TARGET_FREQ,
                            snr_db: snr as f32,
                            dt_sec: 0.0,
                        }];
                        for &(freq, msg) in &case.crowd {
                            sigs.push(simulator::SimSignal {
                                message77: msg,
                                freq_hz: freq,
                                snr_db: case.crowd_snr,
                                dt_sec: 0.0,
                            });
                        }
                        let cfg = simulator::SimConfig { signals: sigs, noise_seed: Some(seed) };
                        let mix = simulator::generate_frame_f32(&cfg);
                        let mut bpf = ButterworthBpf::design(N_POLES, case.bpf_lo, case.bpf_hi, FS);
                        let filt = bpf.filter(&mix);
                        let peak = filt.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
                        let scale = if peak > 1e-6 { 29_000.0 / peak } else { 1.0 };
                        let au: Vec<i16> = filt
                            .iter()
                            .map(|&s| (s * scale).clamp(-32_768.0, 32_767.0) as i16)
                            .collect();
                        let t0 = Instant::now();
                        let hit = decode_sniper_staged(&au, TARGET_FREQ, *depth, 20, EqMode::Local, Some(&ap))
                            .iter()
                            .any(|r| r.message77 == target_msg);
                        let ms = t0.elapsed().as_secs_f64() * 1000.0;
                        (hit as usize, ms)
                    })
                    .reduce(|| (0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
                print!("  {name}:{ok:>3}/{N_SEEDS}({:.0}ms)", ms_sum / N_SEEDS as f64);
            }
            println!();
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────

/// WSJT-X stress test: generate WAVs at our decoder's limit to see if
/// WSJT-X can match.
///
/// Scenario: busy band (15 crowd @ +20 dB) + BPF edge target @ −16 dB.
/// Two WAVs are written:
///   1. `sim_stress_fullband.wav`  — full-band mix (WSJT-X sees this)
///   2. `sim_stress_bpf_edge.wav`  — BPF-filtered (our sniper sees this)
///
/// Our sniper+EQ should decode the target from (2); WSJT-X should fail
/// on (1) due to ADC saturation and may fail on (2) due to BPF edge distortion.
fn run_wsjt_stress_test() {
    use mfsk_core::ft8::message::{pack77_type1, unpack77};
    use bpf::ButterworthBpf;

    // ── Parameters tuned at the sniper+EQ limit ─────────────────────────────
    const TARGET_FREQ: f32 = 1000.0;
    const TARGET_SNR: f32 = -18.0;  // near FT8 threshold
    const CROWD_SNR: f32 = 20.0;
    const N_POLES: usize = 4;
    const FS: f64 = 12_000.0;

    // BPF offset: target at the −3 dB edge
    const BPF_CENTER: f64 = 1250.0; // passband 1000–1500 Hz, target at low edge
    const BPF_LO: f64 = BPF_CENTER - 250.0;
    const BPF_HI: f64 = BPF_CENTER + 250.0;

    let target_msg = pack77_type1("CQ", "3Y0Z", "JD34")
        .expect("failed to pack target message");

    let crowd = crowd_calls_grids();
    let crowd_msgs = build_cq_messages(&crowd);

    println!("=== WSJT-X stress test: crowd @ {CROWD_SNR:+.0} dB, target @ {TARGET_SNR:+.0} dB, BPF edge ===");

    let config = make_busy_band_scenario(
        target_msg,
        TARGET_FREQ,
        TARGET_SNR,
        &crowd_msgs,
        CROWD_SNR,
        Some(555),
    );

    // ── (1) Full-band mixed WAV (WSJT-X test) ──────────────────────────────
    let mix_f32 = simulator::generate_frame_f32(&config);
    let audio_full = simulator::quantise_crowd_agc(&mix_f32, CROWD_SNR, crowd_msgs.len());

    let results_full = decode_frame(
        &audio_full, 200.0, 2800.0, 1.0, None, SHIPPED_DEPTH, 200,
    );
    let target_full = results_full.iter().any(|r| r.message77 == target_msg);
    println!(
        "  [full-band     ] decoded: {:2}  target: {}",
        results_full.len(),
        if target_full { "DECODED" } else { "missed" }
    );

    let out_full = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata").join("sim_stress_fullband.wav");
    let _ = simulator::write_wav(&out_full, &audio_full);
    println!("  WAV: {}", out_full.display());

    // ── (2) BPF-filtered WAV (sniper test) ──────────────────────────────────
    // Apply BPF to the full mix, then re-quantise cleanly.
    let mut bpf = ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
    let filtered = bpf.filter(&mix_f32);
    let peak = filtered.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    let scale = if peak > 1e-6 { 29_000.0 / peak } else { 1.0 };
    let audio_bpf: Vec<i16> = filtered
        .iter()
        .map(|&s| (s * scale).clamp(-32_768.0, 32_767.0) as i16)
        .collect();

    let atten = {
        let bpf_check = ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
        bpf_check.response_db(TARGET_FREQ as f64, FS)
    };

    // Sniper decode: EQ OFF
    let r_off = decode_sniper_eq(&audio_bpf, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Off);
    let t_off = r_off.iter().any(|r| r.message77 == target_msg);

    // Sniper decode: EQ Adaptive
    let r_on = decode_sniper_eq(&audio_bpf, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local);
    let t_on = r_on.iter().any(|r| r.message77 == target_msg);

    println!(
        "  [BPF edge      ] atten={atten:+.1} dB  EQ OFF: {}  EQ Adaptive: {}",
        if t_off { "DECODED" } else { "missed" },
        if t_on { "DECODED" } else { "missed" },
    );
    for r in &r_on {
        if let Some(text) = unpack77(&r.message77) {
            let tag = if r.message77 == target_msg { " ★" } else { "" };
            println!("    {:+5.1} dB  {:7.1} Hz  err={}  {}{tag}", r.snr_db, r.freq_hz, r.hard_errors, text);
        }
    }

    let out_bpf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata").join("sim_stress_bpf_edge.wav");
    let _ = simulator::write_wav(&out_bpf, &audio_bpf);
    println!("  WAV: {}", out_bpf.display());

    // ── (3) Target-only BPF edge WAV (cleanest WSJT-X comparison) ──────────
    // This is what the sniper sees after hardware BPF: just target + noise,
    // no crowd leakage.  WSJT-X decodes this without equalizer; our sniper
    // has the EQ advantage.  Write multiple seeds for the best-case WAV.
    {
        let best_seed = (0u64..20).find(|&seed| {
            let cfg = simulator::SimConfig {
                signals: vec![simulator::SimSignal {
                    message77: target_msg,
                    freq_hz: TARGET_FREQ,
                    snr_db: TARGET_SNR,
                    dt_sec: 0.0,
                }],
                noise_seed: Some(seed),
            };
            let mix = simulator::generate_frame_f32(&cfg);
            let mut bpf = ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
            let filt = bpf.filter(&mix);
            let pk = filt.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
            let sc = if pk > 1e-6 { 29_000.0 / pk } else { 1.0 };
            let au: Vec<i16> = filt.iter().map(|&s| (s * sc).clamp(-32_768.0, 32_767.0) as i16).collect();
            decode_sniper_eq(&au, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local)
                .iter().any(|r| r.message77 == target_msg)
        });

        if let Some(seed) = best_seed {
            let cfg = simulator::SimConfig {
                signals: vec![simulator::SimSignal {
                    message77: target_msg,
                    freq_hz: TARGET_FREQ,
                    snr_db: TARGET_SNR,
                    dt_sec: 0.0,
                }],
                noise_seed: Some(seed),
            };
            let mix = simulator::generate_frame_f32(&cfg);
            let mut bpf = ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
            let filt = bpf.filter(&mix);
            let pk = filt.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
            let sc = if pk > 1e-6 { 29_000.0 / pk } else { 1.0 };
            let au: Vec<i16> = filt.iter().map(|&s| (s * sc).clamp(-32_768.0, 32_767.0) as i16).collect();
            let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("testdata").join("sim_stress_bpf_edge_clean.wav");
            let _ = simulator::write_wav(&out, &au);
            println!(
                "  [target-only   ] BPF edge, seed={seed}  (our sniper+EQ decodes, WSJT-X: ???)"
            );
            println!("  WAV: {}", out.display());
        } else {
            println!("  [target-only   ] no seed found where EQ Adaptive decodes");
        }
    }

    // ── Seed sweep for reliability stats ─────────────────────────────────────
    const N_SEEDS: u64 = 20;
    // ── SNR sweep: find the AP decode threshold ───────────────────────────
    // Realistic AP: only the target callsign is known (not what call1 is)
    let ap = ApHint::new().with_call2("3Y0Z");
    println!("  SNR sweep (BPF edge, {N_SEEDS} seeds each):");
    println!("  {:>6}  {:>8}  {:>8}  {:>8}", "SNR", "EQ OFF", "EQ", "EQ+AP");
    use rayon::prelude::*;
    for snr in [-16, -18, -20, -22, -24, -26] {
        let (ok_off, ok_eq, ok_ap) = (0..N_SEEDS)
            .into_par_iter()
            .map(|seed| {
                let cfg = simulator::SimConfig {
                    signals: vec![simulator::SimSignal {
                        message77: target_msg,
                        freq_hz: TARGET_FREQ,
                        snr_db: snr as f32,
                        dt_sec: 0.0,
                    }],
                    noise_seed: Some(seed),
                };
                let mix = simulator::generate_frame_f32(&cfg);
                let mut bpf = ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
                let filt = bpf.filter(&mix);
                let pk = filt.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
                let sc = if pk > 1e-6 { 29_000.0 / pk } else { 1.0 };
                let au: Vec<i16> = filt.iter().map(|&s| (s * sc).clamp(-32_768.0, 32_767.0) as i16).collect();

                let hit_off = decode_sniper_eq(&au, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Off)
                    .iter().any(|r| r.message77 == target_msg);
                let hit_eq = decode_sniper_eq(&au, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local)
                    .iter().any(|r| r.message77 == target_msg);
                let hit_ap = decode_sniper_ap(&au, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, Some(&ap))
                    .iter().any(|r| r.message77 == target_msg);

                (hit_off as usize, hit_eq as usize, hit_ap as usize)
            })
            .reduce(|| (0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2));
        println!("  {:+4} dB  {:>5}/{N_SEEDS}  {:>5}/{N_SEEDS}  {:>5}/{N_SEEDS}",
            snr, ok_off, ok_eq, ok_ap);
    }
    println!();
}

// ────────────────────────────────────────────────────────────────────────────

/// Speed benchmark: measure decode_frame throughput on a synthetic frame.
///
/// Runs N_WARM warmup iterations (discarded) then N_MEASURE timed iterations.
/// Reports mean, min, and max elapsed time per frame.
///
/// Run with `cargo run --release` for meaningful numbers.
fn run_speed_bench() {
    use std::time::Instant;
    use mfsk_core::ft8::decode::DecodeStrictness;
    use mfsk_core::ft8::message::pack77_type1;

    const N_WARM: usize = 3;
    const N_MEASURE: usize = 10;
    const N_STATIONS: usize = 100;

    // Generate 100 unique CQ messages spread across 200–2800 Hz.
    // Callsign format: JQ1AAA..JQ1ADV (3-letter suffix, all valid in pack28)
    let grids = ["PM95", "PM96", "PM85", "QM06", "QM07", "PM74", "PM84", "PM86", "QN01", "PM75"];
    let mut signals = Vec::with_capacity(N_STATIONS);
    for i in 0..N_STATIONS {
        let c1 = (b'A' + (i / 26) as u8) as char;
        let c2 = (b'A' + (i % 26) as u8) as char;
        let call = format!("JQ1A{c1}{c2}");
        let grid = grids[i % grids.len()];
        let msg = pack77_type1("CQ", &call, grid)
            .unwrap_or_else(|| panic!("pack failed for {call}"));
        let freq = 200.0 + (i as f32 / N_STATIONS as f32) * 2600.0;
        signals.push(simulator::SimSignal {
            message77: msg,
            freq_hz: freq,
            snr_db: 5.0,
            dt_sec: 0.0,
        });
    }
    let config = simulator::SimConfig { signals, noise_seed: Some(42) };
    let audio = simulator::generate_frame(&config);

    println!("=== Speed benchmark: {N_STATIONS} stations, {N_MEASURE} runs (release build recommended) ===");

    // ── Depth matrix on this exact dense (~26 Hz spacing, single-pass) config ──
    //
    // The counter-example that overturned the 2026-07-26 depth-matrix
    // investigation's initial "Minimal always matches Full" conclusion
    // (which was based only on a real recording and narrow-band sniper
    // scenarios, both relatively sparse): here, {Minimal, osd:true} is
    // actively worse than {Full, osd:true} on *both* axes at once —
    // fewer stations decoded AND slower. `SHIPPED_DEPTH` (= `FULL`)
    // reflects this; see its doc comment above for the full story.
    {
        use mfsk_core::ft8::decode::{DecodeDepth, LlrEffort};
        let depths = [
            ("Minimal,noOSD", DecodeDepth { llr_effort: LlrEffort::Minimal, osd: false }),
            ("Minimal,OSD  ", DecodeDepth { llr_effort: LlrEffort::Minimal, osd: true }),
            ("Full,noOSD   ", DecodeDepth { llr_effort: LlrEffort::Full, osd: false }),
            ("Full,OSD     ", DecodeDepth { llr_effort: LlrEffort::Full, osd: true }),
        ];
        println!("  Dense-100-station depth matrix (single-pass):");
        for (name, depth) in depths {
            let t0 = Instant::now();
            let r = decode_frame(&audio, 200.0, 2800.0, 1.0, None, depth, 200);
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            println!("    {name}: decoded={:3}  {ms:7.1} ms", r.len());
        }
    }

    // ── decode_frame (single-pass) ────────────────────────────────────────────
    for _ in 0..N_WARM {
        let _ = decode_frame(&audio, 200.0, 2800.0, 1.0, None, SHIPPED_DEPTH, 200);
    }
    let mut times_single = Vec::with_capacity(N_MEASURE);
    for _ in 0..N_MEASURE {
        let t0 = Instant::now();
        let r = decode_frame(&audio, 200.0, 2800.0, 1.0, None, SHIPPED_DEPTH, 200);
        let elapsed = t0.elapsed();
        times_single.push((elapsed, r.len()));
    }
    let decoded_count = times_single[0].1;
    let ms: Vec<f64> = times_single.iter().map(|(d, _)| d.as_secs_f64() * 1000.0).collect();
    let mean = ms.iter().sum::<f64>() / ms.len() as f64;
    let min  = ms.iter().cloned().fold(f64::INFINITY, f64::min);
    let max  = ms.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    println!("  decode_frame       (decoded={decoded_count:3})  mean={mean:7.1} ms  min={min:7.1} ms  max={max:7.1} ms");

    // ── decode_frame_subtract (3-pass) ────────────────────────────────────────
    for _ in 0..N_WARM {
        let _ = decode_frame_subtract(&audio, 200.0, 2800.0, 1.0, None, SHIPPED_DEPTH, 200, DecodeStrictness::Normal);
    }
    let mut times_sub = Vec::with_capacity(N_MEASURE);
    for _ in 0..N_MEASURE {
        let t0 = Instant::now();
        let r = decode_frame_subtract(&audio, 200.0, 2800.0, 1.0, None, SHIPPED_DEPTH, 200, DecodeStrictness::Normal);
        let elapsed = t0.elapsed();
        times_sub.push((elapsed, r.len()));
    }
    let decoded_sub = times_sub[0].1;
    let ms_sub: Vec<f64> = times_sub.iter().map(|(d, _)| d.as_secs_f64() * 1000.0).collect();
    let mean_s = ms_sub.iter().sum::<f64>() / ms_sub.len() as f64;
    let min_s  = ms_sub.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_s  = ms_sub.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    println!("  decode_frame_sub   (decoded={decoded_sub:3})  mean={mean_s:7.1} ms  min={min_s:7.1} ms  max={max_s:7.1} ms");

    // ── decode_frame_subtract (3-pass), BP_ONLY (no OSD) — candidate for
    // decode_phase2's default, since Phase2 runs every 15s cycle regardless
    // of sniper target ─────────────────────────────────────────────────────
    for _ in 0..N_WARM {
        let _ = decode_frame_subtract(&audio, 200.0, 2800.0, 1.0, None, DecodeDepth::BP_ONLY, 200, DecodeStrictness::Normal);
    }
    let mut times_sub_bp = Vec::with_capacity(N_MEASURE);
    for _ in 0..N_MEASURE {
        let t0 = Instant::now();
        let r = decode_frame_subtract(&audio, 200.0, 2800.0, 1.0, None, DecodeDepth::BP_ONLY, 200, DecodeStrictness::Normal);
        let elapsed = t0.elapsed();
        times_sub_bp.push((elapsed, r.len()));
    }
    let decoded_sub_bp = times_sub_bp[0].1;
    let ms_sub_bp: Vec<f64> = times_sub_bp.iter().map(|(d, _)| d.as_secs_f64() * 1000.0).collect();
    let mean_s_bp = ms_sub_bp.iter().sum::<f64>() / ms_sub_bp.len() as f64;
    let min_s_bp  = ms_sub_bp.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_s_bp  = ms_sub_bp.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    println!("  decode_frame_sub[BP_ONLY] (decoded={decoded_sub_bp:3})  mean={mean_s_bp:7.1} ms  min={min_s_bp:7.1} ms  max={max_s_bp:7.1} ms");

    // ── sniper mode (±250 Hz around 1000 Hz) ──────────────────────────────────
    for _ in 0..N_WARM {
        let _ = decode_sniper_eq(&audio, 1000.0, SHIPPED_DEPTH, 20, EqMode::Local);
    }
    let mut times_sniper = Vec::with_capacity(N_MEASURE);
    for _ in 0..N_MEASURE {
        let t0 = Instant::now();
        let r = decode_sniper_eq(&audio, 1000.0, SHIPPED_DEPTH, 20, EqMode::Local);
        let elapsed = t0.elapsed();
        times_sniper.push((elapsed, r.len()));
    }
    let decoded_sniper = times_sniper[0].1;
    let ms_sn: Vec<f64> = times_sniper.iter().map(|(d, _)| d.as_secs_f64() * 1000.0).collect();
    let mean_sn = ms_sn.iter().sum::<f64>() / ms_sn.len() as f64;
    let min_sn  = ms_sn.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_sn  = ms_sn.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    println!("  sniper+EQ          (decoded={decoded_sniper:3})  mean={mean_sn:7.1} ms  min={min_sn:7.1} ms  max={max_sn:7.1} ms");

    println!("  FT8 period budget: 2400 ms");
    println!();
}

// ────────────────────────────────────────────────────────────────────────────

/// Synthetic +40 dB interference scenario.
///
/// Places a weak target at 1000 Hz (SNR = −5 dB) and a +40 dB interferer at
/// 1200 Hz in the same frame.  Tests that the decoder recovers the target.
fn run_interference_scenario() {
    use mfsk_core::ft8::message::pack77_type1;
    use simulator::{SimConfig, SimSignal, make_interference_scenario};

    println!("=== Synthetic: +40 dB interferer @ 200 Hz offset ===");

    let target_msg = pack77_type1("CQ", "3Y0Z", "JD34")
        .expect("failed to pack target message");
    let interferer_msg = pack77_type1("CQ", "JQ1QSO", "PM95")
        .expect("failed to pack interferer message");

    let config = make_interference_scenario(
        target_msg,
        1000.0,     // target at 1000 Hz
        -5.0,       // target SNR = -5 dB
        interferer_msg,
        1200.0,     // interferer 200 Hz away
        40.0,       // +40 dB above target
        Some(99),
    );

    let audio = simulator::generate_frame(&config);
    let results = decode_frame(&audio, 800.0, 1400.0, 1.0, None, SHIPPED_DEPTH, 50);

    let target_found = results.iter().any(|r| r.message77 == target_msg);
    let interferer_found = results.iter().any(|r| r.message77 == interferer_msg);

    println!(
        "  target   ({:5.1} Hz, SNR {:+.0} dB): {}",
        1000.0_f32,
        -5.0_f32,
        if target_found { "DECODED" } else { "missed" }
    );
    println!(
        "  interferer ({:5.1} Hz, SNR {:+.0} dB): {}",
        1200.0_f32,
        35.0_f32,
        if interferer_found { "DECODED" } else { "missed" }
    );
    println!("  total decoded: {}", results.len());

    // Optionally write the mixed WAV for external WSJT-X verification.
    let out_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("sim_interference.wav");
    if let Ok(()) = simulator::write_wav(&out_path, &audio) {
        println!("  WAV written: {}", out_path.display());
    }

    println!();

    // ── Simulate what sniper mode sees after hardware 500 Hz BPF ────────────
    // After BPF centred on 1000 Hz ±250 Hz, the +40 dB interferer at 1200 Hz
    // is physically removed.  Simulate this by synthesising target only.
    println!("=== Synthetic: sniper mode (interferer outside 500 Hz BPF) ===");

    let config_sniper = SimConfig {
        signals: vec![SimSignal {
            message77: target_msg,
            freq_hz: 1000.0,
            snr_db: -5.0,
            dt_sec: 0.0,
        }],
        noise_seed: Some(99),
    };
    let audio_sniper = simulator::generate_frame(&config_sniper);
    let results_sniper = decode_frame(
        &audio_sniper, 800.0, 1200.0, 0.8, None, SHIPPED_DEPTH, 20,
    );
    let target_sniper = results_sniper.iter().any(|r| r.message77 == target_msg);
    println!(
        "  target   ({:5.1} Hz, SNR {:+.0} dB): {}",
        1000.0_f32,
        -5.0_f32,
        if target_sniper { "DECODED" } else { "missed" }
    );
    println!("  total decoded: {}", results_sniper.len());
}

// ────────────────────────────────────────────────────────────────────────────

/// Extreme limit sweep: find the decoder's breaking point.
///
/// 1. Hard-mixed: crowd +40 dB, sweep target from -14 to -26 dB (full-band subtract)
/// 2. BPF edge: sweep target from -18 to -28 dB (sniper + EQ + AP)
/// 3. Write WAVs at the extreme limit for WSJT-X comparison
fn run_extreme_sweep() {
    use mfsk_core::ft8::decode::{DecodeStrictness, EqMode, ApHint};
    use mfsk_core::ft8::message::pack77_type1;

    let target_msg = pack77_type1("CQ", "3Y0Z", "JD34").unwrap();
    let ap = ApHint::new().with_call2("3Y0Z");
    let crowd_data = crowd_calls_grids();
    const N_SEEDS: u64 = 20;
    const TARGET_FREQ: f32 = 1000.0;

    println!("\n=== EXTREME LIMIT SWEEP ===\n");

    // ── (1) Hard-mixed: crowd +40 dB, target SNR sweep ─────────────────────
    println!("--- Hard-mixed: 15 crowd @ +40 dB, target SNR sweep ({N_SEEDS} seeds) ---");
    println!("  {:>6}  {:>10}  {:>10}", "SNR", "subtract", "sniper+AP");
    use rayon::prelude::*;
    for snr in [-14, -16, -18, -20, -22, -24, -26] {
        let (ok_sub, ok_sniper) = (0..N_SEEDS)
            .into_par_iter()
            .map(|seed| {
                let mut signals: Vec<simulator::SimSignal> = crowd_data.iter().enumerate().map(|(i, (call, grid))| {
                    let msg = pack77_type1("CQ", call, grid).unwrap();
                    simulator::SimSignal {
                        message77: msg,
                        freq_hz: 200.0 + (i as f32 / crowd_data.len() as f32) * 2600.0,
                        snr_db: 40.0,
                        dt_sec: 0.0,
                    }
                }).collect();
                signals.push(simulator::SimSignal {
                    message77: target_msg,
                    freq_hz: TARGET_FREQ,
                    snr_db: snr as f32,
                    dt_sec: 0.0,
                });
                let audio = simulator::generate_frame(&simulator::SimConfig {
                    signals,
                    noise_seed: Some(seed),
                });
                let r_sub = decode_frame_subtract(&audio, 100.0, 3000.0, 1.0, None, SHIPPED_DEPTH, 200, DecodeStrictness::Normal);
                let hit_sub = r_sub.iter().any(|r| r.message77 == target_msg);

                let r_sniper = decode_sniper_ap(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, Some(&ap));
                let hit_sniper = r_sniper.iter().any(|r| r.message77 == target_msg);

                (hit_sub as usize, hit_sniper as usize)
            })
            .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        println!("  {:+4} dB  {:>4}/{:<4}  {:>4}/{:<4}", snr, ok_sub, N_SEEDS, ok_sniper, N_SEEDS);
    }

    // ── (2) BPF edge: target SNR sweep (sniper + EQ + AP) ──────────────────
    // AP with both CQ+call2 (pass 7, 61-bit) and deep passes (9-11, 77-bit)
    println!("\n--- BPF edge (500 Hz, 4-pole): target SNR sweep ({N_SEEDS} seeds) ---");
    println!("  {:>6}  {:>10}  {:>10}  {:>10}  {:>10}", "SNR", "EQ OFF", "EQ", "CQ+call2", "full 77bit");
    const FS: f64 = 12000.0;
    const BPF_LO: f64 = 750.0;
    const BPF_HI: f64 = 1250.0;
    const N_POLES: usize = 4;
    for snr in [-18, -20, -22, -24, -26, -28] {
        // CQ + call2 AP (61-bit, pass 7)
        let ap_cq = ApHint::new().with_call1("CQ").with_call2("3Y0Z");
        // Full AP: simulated "JA1ABC" as mycall (77-bit passes 9-11)
        let ap_full = ApHint::new().with_call1("JA1ABC").with_call2("3Y0Z");
        let (ok_off, ok_eq, ok_ap, ok_full) = (0..N_SEEDS)
            .into_par_iter()
            .map(|seed| {
                let cfg = simulator::SimConfig {
                    signals: vec![simulator::SimSignal {
                        message77: target_msg,
                        freq_hz: TARGET_FREQ,
                        snr_db: snr as f32,
                        dt_sec: 0.0,
                    }],
                    noise_seed: Some(seed),
                };
                let mix = simulator::generate_frame_f32(&cfg);
                let mut bpf = bpf::ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
                let filt = bpf.filter(&mix);
                let pk = filt.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
                let sc = if pk > 1e-6 { 29_000.0 / pk } else { 1.0 };
                let audio: Vec<i16> = filt.iter().map(|&s| (s * sc).clamp(-32_768.0, 32_767.0) as i16).collect();

                let hit_off = decode_sniper_ap(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Off, None)
                    .iter().any(|r| r.message77 == target_msg);
                let hit_eq = decode_sniper_ap(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, None)
                    .iter().any(|r| r.message77 == target_msg);
                let hit_ap = decode_sniper_ap(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, Some(&ap_cq))
                    .iter().any(|r| r.message77 == target_msg);
                let hit_full = decode_sniper_ap(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, Some(&ap_full))
                    .iter().any(|r| r.message77 == target_msg);

                (hit_off as usize, hit_eq as usize, hit_ap as usize, hit_full as usize)
            })
            .reduce(|| (0, 0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2, a.3 + b.3));
        println!("  {:+4} dB  {:>4}/{:<4}  {:>4}/{:<4}  {:>4}/{:<4}  {:>4}/{:<4}", snr, ok_off, N_SEEDS, ok_eq, N_SEEDS, ok_ap, N_SEEDS, ok_full, N_SEEDS);
    }

    // ── (3) Write extreme WAVs for WSJT-X comparison ────────────────────────
    // ── (3) QSO scenario sweep: test 77-bit AP with matching messages ──────
    {
        use mfsk_core::ft8::message::pack77;
        println!("\n--- QSO scenario: BPF edge, EQ+AP ({N_SEEDS} seeds) ---");
        println!("  {:>6}  {:>12}  {:>12}  {:>12}", "SNR", "CQ(61bit)", "REPORT(61b)", "RR73(77bit)");

        // Messages that would occur during a QSO between JA1ABC and 3Y0Z:
        let msg_cq     = pack77_type1("CQ", "3Y0Z", "JD34").unwrap();        // CQ from DX
        let msg_report = pack77("JA1ABC", "3Y0Z", "R-12").unwrap();          // DX sends R-report
        let msg_rr73   = pack77("JA1ABC", "3Y0Z", "RR73").unwrap();          // DX sends RR73

        let ap_cq   = ApHint::new().with_call1("CQ").with_call2("3Y0Z");
        let ap_dir  = ApHint::new().with_call1("JA1ABC").with_call2("3Y0Z"); // 61-bit for directed
        let ap_rr73 = ApHint::new().with_call1("JA1ABC").with_call2("3Y0Z"); // 77-bit (pass 9-11 auto)

        let scenarios: Vec<(&str, [u8; 77], &ApHint)> = vec![
            ("CQ(61bit)",    msg_cq,     &ap_cq),
            ("REPORT(61b)",  msg_report, &ap_dir),
            ("RR73(77bit)",  msg_rr73,   &ap_rr73),
        ];

        println!("  {:>6}  {:>12}  {:>12}  {:>12}", "SNR", "CQ(61bit)", "REPORT(61b)", "RR73(77bit)");
        for snr in [-18, -20, -22, -24, -26] {
            let mut results = Vec::new();
            for (_label, target_msg_qso, ap_hint) in &scenarios {
                let (ok, fp) = (0..N_SEEDS)
                    .into_par_iter()
                    .map(|seed| {
                        let cfg = simulator::SimConfig {
                            signals: vec![simulator::SimSignal {
                                message77: *target_msg_qso,
                                freq_hz: TARGET_FREQ,
                                snr_db: snr as f32,
                                dt_sec: 0.0,
                            }],
                            noise_seed: Some(seed),
                        };
                        let mix = simulator::generate_frame_f32(&cfg);
                        let mut bpf = bpf::ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
                        let filt = bpf.filter(&mix);
                        let pk = filt.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
                        let sc = if pk > 1e-6 { 29_000.0 / pk } else { 1.0 };
                        let audio: Vec<i16> = filt.iter().map(|&s| (s * sc).clamp(-32_768.0, 32_767.0) as i16).collect();
                        let r = decode_sniper_ap(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, Some(ap_hint));
                        let found = r.iter().any(|r| r.message77 == *target_msg_qso);
                        // Count false positives: any decode that isn't the target
                        let fp = r.iter().filter(|r| r.message77 != *target_msg_qso).count();
                        (found as usize, fp)
                    })
                    .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
                let fp_str = if fp > 0 { format!(" FP:{fp}") } else { String::new() };
                results.push(format!("{:>4}/{:<4}{}", ok, N_SEEDS, fp_str));
            }
            println!("  {:+4} dB  {}  {}  {}", snr, results[0], results[1], results[2]);
        }
    }

    // ── (4) Write extreme WAVs for WSJT-X comparison ────────────────────────
    // hard_mixed at -20 dB (near our subtract limit)
    {
        let mut signals: Vec<simulator::SimSignal> = crowd_data.iter().enumerate().map(|(i, (call, grid))| {
            let msg = pack77_type1("CQ", call, grid).unwrap();
            simulator::SimSignal {
                message77: msg,
                freq_hz: 200.0 + (i as f32 / crowd_data.len() as f32) * 2600.0,
                snr_db: 40.0,
                dt_sec: 0.0,
            }
        }).collect();
        signals.push(simulator::SimSignal {
            message77: target_msg,
            freq_hz: TARGET_FREQ,
            snr_db: -20.0,
            dt_sec: 0.0,
        });
        let audio = simulator::generate_frame(&simulator::SimConfig { signals, noise_seed: Some(0) });
        let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata").join("sim_extreme_hard.wav");
        let _ = simulator::write_wav(&out, &audio);
        let r = decode_frame_subtract(&audio, 100.0, 3000.0, 1.0, None, SHIPPED_DEPTH, 200, DecodeStrictness::Normal);
        let found = r.iter().any(|r| r.message77 == target_msg);
        println!("\n  WAV: sim_extreme_hard.wav (crowd +40, target -20)  rs-ft8n: {}  decoded: {}", if found {"3Y0Z FOUND"} else {"3Y0Z missed"}, r.len());
    }

    // BPF edge at -22 dB (near our sniper+EQ+AP limit)
    {
        let cfg = simulator::SimConfig {
            signals: vec![simulator::SimSignal {
                message77: target_msg,
                freq_hz: TARGET_FREQ,
                snr_db: -22.0,
                dt_sec: 0.0,
            }],
            noise_seed: Some(0),
        };
        let mix = simulator::generate_frame_f32(&cfg);
        let mut bpf_f = bpf::ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
        let filt = bpf_f.filter(&mix);
        let pk = filt.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        let sc = if pk > 1e-6 { 29_000.0 / pk } else { 1.0 };
        let audio: Vec<i16> = filt.iter().map(|&s| (s * sc).clamp(-32_768.0, 32_767.0) as i16).collect();
        let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata").join("sim_extreme_edge.wav");
        let _ = simulator::write_wav(&out, &audio);
        let r = decode_sniper_ap(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, Some(&ap));
        let found = r.iter().any(|r| r.message77 == target_msg);
        println!("  WAV: sim_extreme_edge.wav (BPF edge, target -22)  rs-ft8n: {}  decoded: {}", if found {"3Y0Z FOUND"} else {"3Y0Z missed"}, r.len());
    }

    // BPF edge at -24 dB (beyond our limit — test if WSJT-X can still decode)
    {
        let cfg = simulator::SimConfig {
            signals: vec![simulator::SimSignal {
                message77: target_msg,
                freq_hz: TARGET_FREQ,
                snr_db: -24.0,
                dt_sec: 0.0,
            }],
            noise_seed: Some(0),
        };
        let mix = simulator::generate_frame_f32(&cfg);
        let mut bpf_f = bpf::ButterworthBpf::design(N_POLES, BPF_LO, BPF_HI, FS);
        let filt = bpf_f.filter(&mix);
        let pk = filt.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        let sc = if pk > 1e-6 { 29_000.0 / pk } else { 1.0 };
        let audio: Vec<i16> = filt.iter().map(|&s| (s * sc).clamp(-32_768.0, 32_767.0) as i16).collect();
        let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata").join("sim_extreme_edge_24.wav");
        let _ = simulator::write_wav(&out, &audio);
        let r = decode_sniper_ap(&audio, TARGET_FREQ, SHIPPED_DEPTH, 20, EqMode::Local, Some(&ap));
        let found = r.iter().any(|r| r.message77 == target_msg);
        println!("  WAV: sim_extreme_edge_24.wav (BPF edge, target -24)  rs-ft8n: {}  decoded: {}", if found {"3Y0Z FOUND"} else {"3Y0Z missed"}, r.len());
    }
    println!();
}
