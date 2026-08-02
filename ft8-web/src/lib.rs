use wasm_bindgen::prelude::*;
use mfsk_core::ft8::Ft8;
use mfsk_core::ft8::decode::{DecodeStrictness, FftCache};
use mfsk_core::msg::decode_request::DecodeRequest;
use mfsk_core::ft8::hash_table::CallsignHashTable;
use mfsk_core::ft8::message::{unpack77_with_hash, is_plausible_message};
use mfsk_core::ft8::resample::{resample_to_12k, resample_f32_to_12k};
use mfsk_core::ft8::decode_block::{coarse_sync, compute_spectrogram};
use mfsk_core::engine::sync::bootstrap_dt_median;

use std::cell::RefCell;

// FT8/FT4 builders default to `osd: true` and (host-side) a hardcoded
// `LlrEffort::Full` — this build relies on both defaults rather than
// calling `.osd(false)` anywhere.
//
// The 2026-07-26 depth-matrix investigation (`ft8-bench`'s
// `run_depth_matrix_scenario` / `examples/depth_matrix.rs`) tried
// `{llr_effort: Minimal, osd: true}` (back when `LlrEffort` was still
// caller-settable via `DecodeDepth`, pre mfsk-core `.depth()` →
// `.osd(bool)` migration): it matched `Full`'s recall on a real
// recording and on narrow-band sniper crowd scenarios (with a small
// speed win), but on a *dense* synthetic scenario (100 stations across
// 200-2800 Hz, ~26 Hz spacing — narrower than FT8's own ~50 Hz signal
// bandwidth, i.e. real mutual interference between adjacent stations)
// `Minimal+osd` was actively worse than `Full+osd` on **both** axes:
// 96/100 vs 100/100 recall, and 106.9 ms vs 50.4 ms. `Full` never lost
// to `Minimal` in any scenario tested, so there's no evidence-backed
// case for `Minimal` — moot now anyway since the builders hardcode
// `Full` on host. `osd: true` is not in question either way: disabling
// it cost real recall everywhere tested (e.g. -40% messages on the real
// recording) for a cost that scales with candidate count, not audio
// length, so it's cheap even where it matters — never call `.osd(false)`
// here. Device-class shedding (see `ft8-web/www/app.js`'s
// `subDisabledAuto`) should keep targeting SIC (`.sic_rounds(n)`/
// `.sic_early()`), not the OSD toggle.

thread_local! {
    static HASH_TABLE: RefCell<CallsignHashTable> = RefCell::new(CallsignHashTable::new());
    /// Cached resampled audio from Phase 1 (reused by Phase 2).
    static CACHED_AUDIO: RefCell<Option<Vec<i16>>> = RefCell::new(None);
    /// Cached 192k-point FFT from Phase 1 (reused by Phase 2 pass 1).
    static CACHED_FFT: RefCell<Option<FftCache>> = RefCell::new(None);
    /// Phase 1 decode results (passed as `known` to Phase 2).
    static CACHED_PHASE1: RefCell<Vec<mfsk_core::ft8::decode::DecodeResult>> = RefCell::new(Vec::new());
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct DecodedMessage {
    pub freq_hz: f32,
    pub dt_sec: f32,
    pub snr_db: f32,
    pub hard_errors: u32,
    pub pass: u8,
    message: String,
}

#[wasm_bindgen]
impl DecodedMessage {
    #[wasm_bindgen(getter)]
    pub fn message(&self) -> String {
        self.message.clone()
    }
}

fn to_decoded(r: mfsk_core::ft8::decode::DecodeResult) -> Option<DecodedMessage> {
    HASH_TABLE.with(|ht| {
        let ht = ht.borrow();
        let text = unpack77_with_hash(r.message77(), &ht)?;
        if text.is_empty() { return None; }
        if !is_plausible_message(&text) { return None; }
        Some(DecodedMessage {
            freq_hz: r.freq_hz,
            dt_sec: r.dt_sec,
            snr_db: r.snr_db,
            hard_errors: r.hard_errors,
            pass: r.pass,
            message: text,
        })
    })
}

fn register_callsigns(text: &str) {
    HASH_TABLE.with(|ht| {
        let mut ht = ht.borrow_mut();
        for word in text.split_whitespace() {
            if matches!(word, "CQ" | "DE" | "QRZ" | "DX" | "RRR" | "RR73" | "73" | "R" | "") {
                continue;
            }
            if word.starts_with("CQ") { continue; }
            if word.starts_with('<') || word.starts_with('+') || word.starts_with('-')
                || word.starts_with("R+") || word.starts_with("R-") { continue; }
            if word.len() == 4 {
                let b = word.as_bytes();
                if b[0].is_ascii_uppercase() && b[1].is_ascii_uppercase()
                    && b[2].is_ascii_digit() && b[3].is_ascii_digit() {
                    continue;
                }
            }
            if word.starts_with('[') { continue; }
            ht.insert(word);
        }
    });
}

fn to_strictness(level: u8) -> DecodeStrictness {
    match level {
        0 => DecodeStrictness::Strict,
        2 => DecodeStrictness::Deep,
        _ => DecodeStrictness::Normal,
    }
}

/// GUI decode-profile levels (0=Fast/1=Normal/2=Deep) pick both
/// `to_strictness` above and SIC strength: Fast uses a light
/// `.sic_rounds(2)` pass (mfsk-core issues #220/#221 — full `.sic_early()`
/// buys real recall over `.sic_rounds(2)` only against a target masked by
/// a strong same-neighbourhood interferer, which light SIC can't rescue
/// regardless of round count; on generic crowded-band audio rounds(2)
/// already captures most of the gain at a fraction of the cost), Normal/Deep
/// use the full SIC strategy.
fn wants_light_sic(profile: u8) -> bool {
    profile == 0
}

fn decode_and_register(results: Vec<mfsk_core::ft8::decode::DecodeResult>) -> Vec<DecodedMessage> {
    let mut out = Vec::new();
    for r in results {
        if let Some(dm) = to_decoded(r) {
            register_callsigns(&dm.message);
            out.push(dm);
        }
    }
    out
}

/// Decode a 15-second FT8 audio frame (wide-band scan).
///
/// `sample_rate` — input PCM sample rate in Hz (e.g. 12000, 44100, 48000).
/// Non-12 000 Hz input is automatically resampled before decoding.
#[wasm_bindgen]
pub fn decode_wav(samples: &[i16], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = if sample_rate != 12000 { resample_to_12k(samples, sample_rate) } else { samples.to_vec() };
    decode_and_register(
        DecodeRequest::<Ft8>::new(&audio, 100.0, 3000.0, 1.5, 200)
            .strictness(to_strictness(strictness))
            .decode()
            .results
    )
}

/// Sniper-mode decode with multi-pass AP (single WASM call).
///
/// AP passes are handled internally by ft8-core (pass 6-11).
/// The deepest applicable pass is tried first based on available info:
/// Build an ApHint from the supplied AP target fields.
///
/// | callsign | grid | mycall | Hint built                            |
/// |----------|------|--------|---------------------------------------|
/// | empty    | empty| any    | None                                  |
/// | empty    | set  | any    | grid only                             |
/// | set      | any  | empty  | CQ + call2 [+ grid] (Watch phase)     |
/// | set      | any  | set    | mycall + call2       (Call phase)      |
fn build_ap_hint(callsign: &str, grid: &str, mycall: &str) -> Option<mfsk_core::ft8::decode::ApHint> {
    use mfsk_core::ft8::decode::ApHint;
    if callsign.is_empty() && grid.is_empty() {
        None
    } else if callsign.is_empty() {
        // grid only (no call known yet)
        Some(ApHint::new().with_grid(grid))
    } else if mycall.is_empty() {
        // Watch phase: CQ-style hint; add grid for stronger lock
        let mut h = ApHint::new().with_call1("CQ").with_call2(callsign);
        if !grid.is_empty() { h = h.with_grid(grid); }
        Some(h)
    } else {
        // Call phase: QSO hint (grid ignored — bits overlap with report field)
        Some(ApHint::new().with_call1(mycall).with_call2(callsign))
    }
}

///   mycall + dxcall + RRR/RR73/73 → 77-bit lock (passes 9-11)
///   CQ + dxcall + grid → up to 76-bit lock (passes 7/8)
///   mycall + dxcall → 61-bit lock (pass 8)
///   dxcall only → 33-bit lock (pass 6)
///   grid only → 15-bit lock (pass 6 fallback)
///
/// Pass `mycall = ""` for Watch phase (CQ-style hint + grid).
/// Pass `mycall = <own_call>` for Call phase (QSO hint, grid ignored).
#[wasm_bindgen]
pub fn decode_sniper(samples: &[i16], target_freq: f32, callsign: &str, grid: &str, mycall: &str, eq_on: bool, sample_rate: u32) -> Vec<DecodedMessage> {
    use mfsk_core::ft8::decode::EqMode;

    let eq_mode = if eq_on { EqMode::Local } else { EqMode::Off };
    let ap = build_ap_hint(callsign, grid, mycall);
    let audio = if sample_rate != 12000 { resample_to_12k(samples, sample_rate) } else { samples.to_vec() };
    decode_and_register(sniper_decode(&audio, target_freq, eq_mode, ap.as_ref()))
}

/// Narrow-band (±250 Hz around `target_freq`) sniper decode: the modern
/// staged-checkpoint SIC engine (issue #180/#191, `.sic_early()` since
/// mfsk-core #218) applied to the BPF passband, with the adaptive
/// equalizer wired through it (see mfsk-core commit fe286cc — the
/// pre-#218 `.staged()`/`.flat()` used to silently drop `eq_mode`).
/// Replaces the old `decode_sniper_sic` (deleted upstream, benchmarked
/// as strictly worse than this combination — see `docs/bench.md`).
fn sniper_decode(
    audio: &[i16],
    target_freq: f32,
    eq_mode: mfsk_core::ft8::decode::EqMode,
    ap: Option<&mfsk_core::ft8::decode::ApHint>,
) -> Vec<mfsk_core::ft8::decode::DecodeResult> {
    let freq_min = (target_freq - 250.0).max(100.0);
    let freq_max = (target_freq + 250.0).min(5900.0);
    let mut req = DecodeRequest::<Ft8>::new(audio, freq_min, freq_max, 0.8, 20)
        .eq_mode(eq_mode);
    if let Some(ap) = ap {
        req = req.ap_hint(ap);
    }
    req.sic_early().decode().results
}

#[wasm_bindgen]
pub fn encode_ft8(call1: &str, call2: &str, report: &str, freq_hz: f32) -> Result<Vec<f32>, JsValue> {
    use mfsk_core::ft8::message::pack77;
    use mfsk_core::ft8::wave_gen::{message_to_tones, tones_to_f32};

    let msg77 = pack77(call1, call2, report)
        .ok_or_else(|| JsValue::from_str("Failed to pack message"))?;
    let tones = message_to_tones(&msg77);
    Ok(tones_to_f32(&tones, freq_hz, 1.0))
}

/// Encode a free-text FT8 message (Type 0, n3=0) as audio samples.
///
/// `text` — up to 13 characters from the FT8 free-text alphabet.
#[wasm_bindgen]
pub fn encode_free_text(text: &str, freq_hz: f32) -> Result<Vec<f32>, JsValue> {
    use mfsk_core::ft8::message::pack77_free_text;
    use mfsk_core::ft8::wave_gen::{message_to_tones, tones_to_f32};

    let msg77 = pack77_free_text(text)
        .ok_or_else(|| JsValue::from_str("Invalid free text (max 13 chars, 0-9 A-Z +-./?)"))?;
    let tones = message_to_tones(&msg77);
    Ok(tones_to_f32(&tones, freq_hz, 1.0))
}

/// Decode with iterative signal subtraction.
///
/// `sample_rate` — input PCM sample rate in Hz. Non-12 000 Hz input is
/// automatically resampled before decoding.
#[wasm_bindgen]
pub fn decode_wav_subtract(samples: &[i16], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = if sample_rate != 12000 { resample_to_12k(samples, sample_rate) } else { samples.to_vec() };
    decode_and_register(
        DecodeRequest::<Ft8>::new(&audio, 100.0, 3000.0, 1.0, 200)
            .strictness(to_strictness(strictness))
            .sic_early()
            .decode()
            .results
    )
}

// ──────────────────────────────────────────────────────────────────────────
// f32 entry points — used by the live AudioWorklet path so the JS side can
// pass a Float32Array directly without an intermediate i16 conversion loop
// (which on Atom-class CPUs costs ~5-10 ms per period and is pure waste).
// The conversion + resample + clamp now happens in one Rust pass.
// ──────────────────────────────────────────────────────────────────────────

/// f32 variant of `decode_wav`. See `decode_wav` for parameters.
#[wasm_bindgen]
pub fn decode_wav_f32(samples: &[f32], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = resample_f32_to_12k(samples, sample_rate);
    decode_and_register(
        DecodeRequest::<Ft8>::new(&audio, 100.0, 3000.0, 1.5, 200)
            .strictness(to_strictness(strictness))
            .decode()
            .results
    )
}

/// f32 variant of `decode_wav_subtract`. See `decode_wav_subtract` for parameters.
#[wasm_bindgen]
pub fn decode_wav_subtract_f32(samples: &[f32], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = resample_f32_to_12k(samples, sample_rate);
    decode_and_register(
        DecodeRequest::<Ft8>::new(&audio, 100.0, 3000.0, 1.0, 200)
            .strictness(to_strictness(strictness))
            .sic_early()
            .decode()
            .results
    )
}

/// Cold-start DT estimate from `coarse_sync` candidates.
///
/// Returns the DT median of the top-5 highest-score coarse-sync candidates
/// (mfsk-core 0.6.6 `bootstrap_dt_median`), which lands within ±100 ms of
/// the confirmed-decode DT median on reference recordings — useful for
/// seeding the JS-side period manager when the device clock is skewed >2 s
/// from UTC and no confirmed decode can be obtained yet.
///
/// Returns `None` (→ `undefined` in JS) when no candidates are found.
#[wasm_bindgen]
pub fn bootstrap_dt_f32(samples: &[f32], sample_rate: u32) -> Option<f32> {
    let audio = resample_f32_to_12k(samples, sample_rate);
    let spec = compute_spectrogram(&audio, 3000.0);
    let cands = coarse_sync(&spec, 100.0, 3000.0, 1.0, 200);
    bootstrap_dt_median(&cands, 5)
}

/// i16 variant of [`bootstrap_dt_f32`].
#[wasm_bindgen]
pub fn bootstrap_dt(samples: &[i16], sample_rate: u32) -> Option<f32> {
    let audio = if sample_rate != 12000 { resample_to_12k(samples, sample_rate) } else { samples.to_vec() };
    let spec = compute_spectrogram(&audio, 3000.0);
    let cands = coarse_sync(&spec, 100.0, 3000.0, 1.0, 200);
    bootstrap_dt_median(&cands, 5)
}

/// f32 variant of `decode_sniper`. See `decode_sniper` for parameters.
#[wasm_bindgen]
pub fn decode_sniper_f32(samples: &[f32], target_freq: f32, callsign: &str, grid: &str, mycall: &str, eq_on: bool, sample_rate: u32) -> Vec<DecodedMessage> {
    use mfsk_core::ft8::decode::EqMode;

    let eq_mode = if eq_on { EqMode::Local } else { EqMode::Off };
    let ap = build_ap_hint(callsign, grid, mycall);
    let audio = resample_f32_to_12k(samples, sample_rate);
    decode_and_register(sniper_decode(&audio, target_freq, eq_mode, ap.as_ref()))
}

// ──────────────────────────────────────────────────────────────────────────
// Pipelined decode: Phase 1 (fast) + Phase 2 (deep subtract)
//
// Phase 1 decodes strong signals quickly and caches audio + FFT in
// thread_local storage.  Phase 2 reuses the cache, runs 3-pass subtract,
// and returns only the newly decoded messages.
// ──────────────────────────────────────────────────────────────────────────

/// Phase 1 decode (i16): fast single-pass decode.
///
/// Caches the resampled audio and FFT for a subsequent `decode_phase2` call.
#[wasm_bindgen]
pub fn decode_phase1(samples: &[i16], sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = if sample_rate != 12000 { resample_to_12k(samples, sample_rate) } else { samples.to_vec() };
    let outcome = DecodeRequest::<Ft8>::new(&audio, 100.0, 3000.0, 1.5, 200)
        .decode();
    CACHED_AUDIO.with(|a| *a.borrow_mut() = Some(audio));
    CACHED_FFT.with(|f| *f.borrow_mut() = Some(outcome.fft_cache));
    CACHED_PHASE1.with(|p| *p.borrow_mut() = outcome.results.clone());
    decode_and_register(outcome.results)
}

/// Phase 2 decode (i16): SIC using cached Phase 1 state, strength picked by
/// the GUI decode-profile level (see `wants_light_sic`).
///
/// Panics if `decode_phase1` was not called first. Prior to mfsk-core
/// commit fe286cc / issue #191, this call went through a separate,
/// unfixed flat-3-pass engine (`decode_frame_subtract_with_known`) that
/// never received the staged-checkpoint SIC recall improvements
/// `decode_wav_subtract` got — `known`/`fft_cache` are now honoured
/// directly by `.sic_early()` (renamed from `.staged()` in mfsk-core
/// #218), so this is the same engine as every other subtract path.
#[wasm_bindgen]
pub fn decode_phase2(profile: u8) -> Vec<DecodedMessage> {
    let audio = CACHED_AUDIO.with(|a| a.borrow_mut().take())
        .expect("decode_phase1 must run first");
    let fft = CACHED_FFT.with(|f| f.borrow_mut().take());
    let known = CACHED_PHASE1.with(|p| std::mem::take(&mut *p.borrow_mut()));
    let mut req = DecodeRequest::<Ft8>::new(&audio, 100.0, 3000.0, 1.0, 200)
        .strictness(to_strictness(profile))
        .known(&known);
    if let Some(fft) = fft {
        req = req.fft_cache(fft);
    }
    req = if wants_light_sic(profile) { req.sic_rounds(2) } else { req.sic_early() };
    decode_and_register(req.decode().results)
}

/// Phase 1 decode (f32): fast single-pass decode for live AudioWorklet path.
///
/// Caches the resampled audio and FFT for a subsequent `decode_phase2_f32` call.
#[wasm_bindgen]
pub fn decode_phase1_f32(samples: &[f32], sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = resample_f32_to_12k(samples, sample_rate);
    let outcome = DecodeRequest::<Ft8>::new(&audio, 100.0, 3000.0, 1.5, 200)
        .decode();
    CACHED_AUDIO.with(|a| *a.borrow_mut() = Some(audio));
    CACHED_FFT.with(|f| *f.borrow_mut() = Some(outcome.fft_cache));
    CACHED_PHASE1.with(|p| *p.borrow_mut() = outcome.results.clone());
    decode_and_register(outcome.results)
}

/// Phase 2 decode (f32): SIC using cached Phase 1 state, strength picked by
/// the GUI decode-profile level (see `wants_light_sic`).
///
/// Panics if `decode_phase1_f32` was not called first. See `decode_phase2`
/// for why this now shares the same staged-checkpoint SIC engine as
/// `decode_wav_subtract_f32`.
#[wasm_bindgen]
pub fn decode_phase2_f32(profile: u8) -> Vec<DecodedMessage> {
    let audio = CACHED_AUDIO.with(|a| a.borrow_mut().take())
        .expect("decode_phase1_f32 must run first");
    let fft = CACHED_FFT.with(|f| f.borrow_mut().take());
    let known = CACHED_PHASE1.with(|p| std::mem::take(&mut *p.borrow_mut()));
    let mut req = DecodeRequest::<Ft8>::new(&audio, 100.0, 3000.0, 1.0, 200)
        .strictness(to_strictness(profile))
        .known(&known);
    if let Some(fft) = fft {
        req = req.fft_cache(fft);
    }
    req = if wants_light_sic(profile) { req.sic_rounds(2) } else { req.sic_early() };
    decode_and_register(req.decode().results)
}

// ──────────────────────────────────────────────────────────────────────────
// FT4 entry points
//
// FT4 reuses the 77-bit WSJT message format (same DecodedMessage struct,
// same callsign hash table) and LDPC(174,91) FEC as FT8, so the JS side
// needs no shape changes beyond a protocol switch. Slot length (7.5 s vs
// 15 s) and mid-band downsample are handled inside ft4-core.
// ──────────────────────────────────────────────────────────────────────────

fn ft4_to_decoded(r: mfsk_core::ft4::decode::DecodeResult) -> Option<DecodedMessage> {
    HASH_TABLE.with(|ht| {
        let ht = ht.borrow();
        let msg77: [u8; 77] = r.message77().try_into().expect("77-bit message slice");
        let text = mfsk_core::msg::wsjt77::unpack77_with_hash(&msg77, &ht)?;
        if text.is_empty() || !mfsk_core::msg::wsjt77::is_plausible_message(&text) {
            return None;
        }
        Some(DecodedMessage {
            freq_hz: r.freq_hz,
            dt_sec: r.dt_sec,
            snr_db: r.snr_db,
            hard_errors: r.hard_errors,
            pass: r.pass,
            message: text,
        })
    })
}

fn ft4_decode_and_register(results: Vec<mfsk_core::ft4::decode::DecodeResult>) -> Vec<DecodedMessage> {
    let mut out = Vec::new();
    for r in results {
        if let Some(dm) = ft4_to_decoded(r) {
            register_callsigns(&dm.message);
            out.push(dm);
        }
    }
    out
}

/// Decode a 7.5-second FT4 slot (wide-band scan). Non-12 kHz input is
/// resampled automatically.
#[wasm_bindgen]
pub fn decode_ft4_wav(samples: &[i16], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = if sample_rate != 12000 {
        resample_to_12k(samples, sample_rate)
    } else {
        samples.to_vec()
    };
    ft4_decode_and_register(
        DecodeRequest::<mfsk_core::ft4::Ft4>::new(&audio, 300.0, 2700.0, 1.2, 50)
            .strictness(to_strictness(strictness))
            .decode()
            .results,
    )
}

/// f32 variant of [`decode_ft4_wav`].
#[wasm_bindgen]
pub fn decode_ft4_wav_f32(samples: &[f32], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = resample_f32_to_12k(samples, sample_rate);
    ft4_decode_and_register(
        DecodeRequest::<mfsk_core::ft4::Ft4>::new(&audio, 300.0, 2700.0, 1.2, 50)
            .strictness(to_strictness(strictness))
            .decode()
            .results,
    )
}

/// FT4 multi-pass subtract decode (SIC) for crowded slots. `profile`
/// (0=Fast/1=Normal/2=Deep) picks both strictness and SIC round count —
/// 2 rounds for Fast, 3 (full) for Normal/Deep (see `wants_light_sic`).
#[wasm_bindgen]
pub fn decode_ft4_wav_subtract(samples: &[i16], profile: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = if sample_rate != 12000 {
        resample_to_12k(samples, sample_rate)
    } else {
        samples.to_vec()
    };
    ft4_decode_and_register(
        DecodeRequest::<mfsk_core::ft4::Ft4>::new(&audio, 300.0, 2700.0, 1.2, 50)
            .strictness(to_strictness(profile))
            .sic_rounds(if wants_light_sic(profile) { 2 } else { 3 })
            .decode()
            .results,
    )
}

/// f32 variant of [`decode_ft4_wav_subtract`].
#[wasm_bindgen]
pub fn decode_ft4_wav_subtract_f32(samples: &[f32], profile: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = resample_f32_to_12k(samples, sample_rate);
    ft4_decode_and_register(
        DecodeRequest::<mfsk_core::ft4::Ft4>::new(&audio, 300.0, 2700.0, 1.2, 50)
            .strictness(to_strictness(profile))
            .sic_rounds(if wants_light_sic(profile) { 2 } else { 3 })
            .decode()
            .results,
    )
}

/// FT4 sniper-mode decode at a target frequency with optional AP hints.
#[wasm_bindgen]
pub fn decode_ft4_sniper(
    samples: &[i16],
    target_freq: f32,
    callsign: &str,
    mycall: &str,
    eq_on: bool,
    sample_rate: u32,
) -> Vec<DecodedMessage> {
    use mfsk_core::ft4::decode::ApHint;
    use mfsk_core::engine::equalize::EqMode;
    use mfsk_core::msg::decode_request::SniperRequest;

    let eq_mode = if eq_on { EqMode::Local } else { EqMode::Off };
    let ap = if callsign.is_empty() {
        None
    } else if mycall.is_empty() {
        Some(ApHint::new().with_call1("CQ").with_call2(callsign))
    } else {
        Some(ApHint::new().with_call1(mycall).with_call2(callsign))
    };

    let audio = if sample_rate != 12000 {
        resample_to_12k(samples, sample_rate)
    } else {
        samples.to_vec()
    };
    let mut req = SniperRequest::<mfsk_core::ft4::Ft4>::new(&audio, target_freq, 15)
        .eq_mode(eq_mode);
    if let Some(ap) = ap.as_ref() {
        req = req.ap_hint(ap);
    }
    ft4_decode_and_register(req.decode().results)
}

/// f32 variant of [`decode_ft4_sniper`].
#[wasm_bindgen]
pub fn decode_ft4_sniper_f32(
    samples: &[f32],
    target_freq: f32,
    callsign: &str,
    mycall: &str,
    eq_on: bool,
    sample_rate: u32,
) -> Vec<DecodedMessage> {
    use mfsk_core::ft4::decode::ApHint;
    use mfsk_core::engine::equalize::EqMode;
    use mfsk_core::msg::decode_request::SniperRequest;

    let eq_mode = if eq_on { EqMode::Local } else { EqMode::Off };
    let ap = if callsign.is_empty() {
        None
    } else if mycall.is_empty() {
        Some(ApHint::new().with_call1("CQ").with_call2(callsign))
    } else {
        Some(ApHint::new().with_call1(mycall).with_call2(callsign))
    };

    let audio = resample_f32_to_12k(samples, sample_rate);
    let mut req = SniperRequest::<mfsk_core::ft4::Ft4>::new(&audio, target_freq, 15)
        .eq_mode(eq_mode);
    if let Some(ap) = ap.as_ref() {
        req = req.ap_hint(ap);
    }
    ft4_decode_and_register(req.decode().results)
}

/// Encode an FT4 standard message (CALL1 CALL2 GRID/REPORT) as 12 kHz PCM.
#[wasm_bindgen]
pub fn encode_ft4(call1: &str, call2: &str, report: &str, freq_hz: f32) -> Result<Vec<f32>, JsValue> {
    use mfsk_core::msg::wsjt77::pack77;
    let msg77 = pack77(call1, call2, report).ok_or_else(|| JsValue::from_str("Failed to pack message"))?;
    let tones = mfsk_core::ft4::encode::message_to_tones(&msg77);
    Ok(mfsk_core::ft4::encode::tones_to_f32(&tones, freq_hz, 1.0))
}

/// Encode a free-text FT4 message (up to 13 chars from the FT8 alphabet).
#[wasm_bindgen]
pub fn encode_ft4_free_text(text: &str, freq_hz: f32) -> Result<Vec<f32>, JsValue> {
    use mfsk_core::msg::wsjt77::pack77_free_text;
    let msg77 = pack77_free_text(text).ok_or_else(|| JsValue::from_str("Invalid free text"))?;
    let tones = mfsk_core::ft4::encode::message_to_tones(&msg77);
    Ok(mfsk_core::ft4::encode::tones_to_f32(&tones, freq_hz, 1.0))
}

// ───────────────────────────────────────────────────────────────────────
// FST4 — five wired sub-modes (FST4-15 / -30 / -60 / -120 / -300)
//
// Sub-mode encoding (T/R period):
//   0 = FST4-15  (15 s)    1 = FST4-30  (30 s)    2 = FST4-60  (60 s)
//   3 = FST4-120 (120 s)   4 = FST4-300 (300 s)
//
// FST4 shares FT4's generic `DecodeRequest` / `DecodeResult` (mfsk-core
// #194), so decoded frames flow through the same `ft4_decode_and_register`
// path. Frame coarse-sync finds the time offset internally, so the WAV
// path only needs the full-slot audio — no nominal-start hint (unlike
// Q65). FST4 SIC (subtract, issue #193) and wide-band AP are not
// implemented upstream, so only the plain wide-band scan is exposed.
// ───────────────────────────────────────────────────────────────────────

// Audio passband + coarse-sync tuning for the FST4 wide-band scan.
// Seeded from FT4's operating point; revisit with ft8-bench sweeps.
const FST4_FLOW: f32 = 100.0;
const FST4_FHIGH: f32 = 3000.0;
const FST4_SYNC_MIN: f32 = 1.2;
const FST4_MAX_CAND: usize = 50;

macro_rules! dispatch_fst4_submode {
    ($submode:expr, $body:ident) => {
        match $submode {
            0 => $body!(mfsk_core::fst4::Fst4s15),
            1 => $body!(mfsk_core::fst4::Fst4s30),
            2 => $body!(mfsk_core::fst4::Fst4s60),
            3 => $body!(mfsk_core::fst4::Fst4s120),
            4 => $body!(mfsk_core::fst4::Fst4s300),
            _ => Vec::new(),
        }
    };
}

/// Decode an FST4 slot (wide-band scan). `submode` 0..=4 picks the T/R
/// period (15/30/60/120/300 s); `profile` (0=Fast/1=Normal/2=Deep) maps
/// to `DecodeStrictness`. Non-12 kHz input is auto-resampled.
#[wasm_bindgen]
pub fn decode_fst4_wav(samples: &[i16], submode: u8, profile: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = if sample_rate != 12000 {
        resample_to_12k(samples, sample_rate)
    } else {
        samples.to_vec()
    };
    macro_rules! scan_body {
        ($p:ty) => {
            ft4_decode_and_register(
                DecodeRequest::<$p>::new(&audio, FST4_FLOW, FST4_FHIGH, FST4_SYNC_MIN, FST4_MAX_CAND)
                    .strictness(to_strictness(profile))
                    .decode()
                    .results,
            )
        };
    }
    dispatch_fst4_submode!(submode, scan_body)
}

/// f32 variant of [`decode_fst4_wav`].
#[wasm_bindgen]
pub fn decode_fst4_wav_f32(samples: &[f32], submode: u8, profile: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let audio = resample_f32_to_12k(samples, sample_rate);
    macro_rules! scan_body {
        ($p:ty) => {
            ft4_decode_and_register(
                DecodeRequest::<$p>::new(&audio, FST4_FLOW, FST4_FHIGH, FST4_SYNC_MIN, FST4_MAX_CAND)
                    .strictness(to_strictness(profile))
                    .decode()
                    .results,
            )
        };
    }
    dispatch_fst4_submode!(submode, scan_body)
}

/// Encode a standard FST4 message (CALL1 CALL2 GRID/REPORT) at the
/// requested sub-mode + audio centre frequency. `submode` 0..=4 picks
/// the T/R period (15/30/60/120/300 s), which only changes the GFSK
/// pulse-shaping constant — the 77-bit message packing and tone
/// sequence are sub-mode independent (shared with FT4/FT8). Returns
/// 12 kHz f32 PCM at amplitude 1.0.
#[wasm_bindgen]
pub fn encode_fst4(call1: &str, call2: &str, report: &str, freq_hz: f32, submode: u8) -> Result<Vec<f32>, JsValue> {
    use mfsk_core::fst4::encode::{
        message_to_tones, tones_to_f32_with_gfsk, FST4_120_GFSK, FST4_15_GFSK, FST4_300_GFSK, FST4_30_GFSK,
        FST4_60A_GFSK,
    };
    use mfsk_core::msg::wsjt77::pack77;
    let msg77 = pack77(call1, call2, report).ok_or_else(|| JsValue::from_str("Failed to pack message"))?;
    let tones = message_to_tones(&msg77);
    let cfg = match submode {
        0 => &FST4_15_GFSK,
        1 => &FST4_30_GFSK,
        2 => &FST4_60A_GFSK,
        3 => &FST4_120_GFSK,
        4 => &FST4_300_GFSK,
        _ => return Err(JsValue::from_str("Invalid FST4 sub-mode (expected 0..=4)")),
    };
    Ok(tones_to_f32_with_gfsk(&tones, freq_hz, 1.0, cfg))
}

// ───────────────────────────────────────────────────────────────────────
// WSPR
// ───────────────────────────────────────────────────────────────────────

fn wspr_decode_to_messages(decodes: Vec<mfsk_core::wspr::WsprResult>) -> Vec<DecodedMessage> {
    decodes
        .into_iter()
        .map(|d| DecodedMessage {
            freq_hz: d.freq_hz,
            // dt_sec reported relative to the 12 kHz pipeline (post-resample).
            dt_sec: d.start_sample as f32 / 12_000.0,
            snr_db: 0.0,
            hard_errors: 0,
            pass: 0,
            message: d.message.to_string(),
        })
        .collect()
}

/// Decode a 120-s WSPR slot. Non-12 kHz input is auto-resampled. Runs
/// coarse (freq, time) search with the default time tolerance and
/// 1400-1600 Hz freq sweep, then Fano-decodes every candidate above
/// the sync-score threshold.
#[wasm_bindgen]
pub fn decode_wspr_wav(samples: &[i16], sample_rate: u32) -> Vec<DecodedMessage> {
    use mfsk_core::engine::dsp::resample::resample_i16_to_12k_f32;
    let audio = resample_i16_to_12k_f32(samples, sample_rate);
    let decodes = mfsk_core::wspr::decode::decode_scan_default(&audio, 12_000);
    wspr_decode_to_messages(decodes)
}

/// f32 variant of [`decode_wspr_wav`].
#[wasm_bindgen]
pub fn decode_wspr_wav_f32(samples: &[f32], sample_rate: u32) -> Vec<DecodedMessage> {
    use mfsk_core::engine::dsp::resample::resample_f32_to_12k_f32;
    let audio = resample_f32_to_12k_f32(samples, sample_rate);
    let decodes = mfsk_core::wspr::decode::decode_scan_default(&audio, 12_000);
    wspr_decode_to_messages(decodes)
}

/// Encode a Type-1 WSPR message ("CALLSIGN GRID4 POWER_DBM") as 12 kHz
/// PCM audio suitable for transmission.
#[wasm_bindgen]
pub fn encode_wspr(
    callsign: &str,
    grid: &str,
    power_dbm: i32,
    freq_hz: f32,
) -> Result<Vec<f32>, JsValue> {
    mfsk_core::wspr::synthesize_type1(callsign, grid, power_dbm, 12_000, freq_hz, 0.3)
        .ok_or_else(|| JsValue::from_str("Invalid WSPR message (bad callsign/grid/power)"))
}

// ───────────────────────────────────────────────────────────────────────
// Q65 — six wired sub-modes (Q65-30A + Q65-60A‥E)
//
// Sub-mode encoding (matches `MfskQ65SubMode` in mfsk-ffi):
//   0 = Q65-30A  (30 s slot, ×1 spacing, terrestrial / ionoscatter)
//   1 = Q65-60A  (60 s slot, ×1 spacing, 6 m EME)
//   2 = Q65-60B  (60 s slot, ×2 spacing, 70 cm / 23 cm EME)
//   3 = Q65-60C  (60 s slot, ×4 spacing, ~3 GHz microwave EME)
//   4 = Q65-60D  (60 s slot, ×8 spacing, 5.7 / 10 GHz EME)
//   5 = Q65-60E  (60 s slot, ×16 spacing, 24 GHz+ / extreme spread)
//
// `decode_q65_*` returns `DecodedMessage` with `dt_sec = start_sample /
// 12_000` and `hard_errors = QRA BP iterations consumed`. SNR is left
// at 0.0 (Q65 doesn't report a comparable SNR through this surface).
// ───────────────────────────────────────────────────────────────────────

fn q65_decodes_to_messages(decodes: Vec<mfsk_core::q65::Q65Result>) -> Vec<DecodedMessage> {
    decodes
        .into_iter()
        .map(|d| DecodedMessage {
            freq_hz: d.freq_hz,
            dt_sec: d.start_sample as f32 / 12_000.0,
            snr_db: d.snr_db,
            hard_errors: d.iterations,
            pass: 0,
            message: d.message,
        })
        .collect()
}

macro_rules! dispatch_q65_submode {
    ($submode:expr, $body:ident) => {
        match $submode {
            0 => $body!(mfsk_core::q65::Q65a30),
            1 => $body!(mfsk_core::q65::Q65a60),
            2 => $body!(mfsk_core::q65::Q65b60),
            3 => $body!(mfsk_core::q65::Q65c60),
            4 => $body!(mfsk_core::q65::Q65d60),
            5 => $body!(mfsk_core::q65::Q65e60),
            _ => Vec::new(),
        }
    };
}

/// Wide-tolerance search params for offline WAV decode. The
/// `SearchParams::default()` lives at the live-audio operating point
/// (±1.5 s around a known UTC-aligned slot start, 8 candidates) — it
/// returns nothing on a WAV-drop where the signal can begin anywhere
/// in the slot. Mirror the `q65_wsjtx_samples` test config: scan the
/// full slot, lower the score threshold so weak ionoscatter / EME
/// signals reach the BP / fast-fading metric.
fn q65_wav_search_params() -> mfsk_core::q65::search::SearchParams {
    mfsk_core::q65::search::SearchParams {
        freq_min_hz: 200.0,
        freq_max_hz: 3_000.0,
        // ±50 symbols × 0.3 s/sym ≈ ±15 s — covers the full 30 s slot
        // for Q65-30A; for Q65-60* we centre on the slot midpoint
        // below so this still captures the whole 60 s slot.
        time_tolerance_symbols: 50,
        score_threshold: 0.05,
        max_candidates: 32,
    }
}

/// Slot-midpoint nominal-start sample for the WSJT-X reference WAVs:
/// Q65-30A is 30 s (midpoint = 15 s × 12 kHz), Q65-60A‥E are 60 s
/// (midpoint = 30 s × 12 kHz). With `time_tolerance_symbols = 50`
/// (±15 s) the search window then covers the full slot in both cases.
fn q65_slot_midpoint_samples(submode: u8) -> usize {
    match submode {
        0 => 12_000 * 15,
        _ => 12_000 * 30,
    }
}

/// Plain Q65 BP decode (basic AWGN strategy). f32 audio.
#[wasm_bindgen]
pub fn decode_q65_wav_f32(samples: &[f32], submode: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    use mfsk_core::engine::dsp::resample::resample_f32_to_12k_f32;
    let audio = resample_f32_to_12k_f32(samples, sample_rate);
    let params = q65_wav_search_params();
    let nominal_mid = q65_slot_midpoint_samples(submode);
    macro_rules! scan_body {
        ($p:ty) => {
            mfsk_core::q65::DecodeRequest::<$p>::new(&audio, 12_000, nominal_mid, params).decode()
        };
    }
    let decodes = dispatch_q65_submode!(submode, scan_body);
    q65_decodes_to_messages(decodes)
}

/// Plain Q65 BP decode. i16 audio variant.
#[wasm_bindgen]
pub fn decode_q65_wav(samples: &[i16], submode: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    use mfsk_core::engine::dsp::resample::resample_i16_to_12k_f32;
    let audio = resample_i16_to_12k_f32(samples, sample_rate);
    let params = q65_wav_search_params();
    let nominal_mid = q65_slot_midpoint_samples(submode);
    macro_rules! scan_body {
        ($p:ty) => {
            mfsk_core::q65::DecodeRequest::<$p>::new(&audio, 12_000, nominal_mid, params).decode()
        };
    }
    let decodes = dispatch_q65_submode!(submode, scan_body);
    q65_decodes_to_messages(decodes)
}

/// Q65 fast-fading metric decode (high-Doppler EME).
///
/// `b90_ts` is the spread-bandwidth × symbol-period dimensionless
/// product. Calibrated test values: 3 (light spread), 8 (moderate),
/// 15 (heavy / 10+ GHz EME). `model`: 0 = Gaussian, 1 = Lorentzian.
#[wasm_bindgen]
pub fn decode_q65_wav_fading_f32(
    samples: &[f32],
    submode: u8,
    b90_ts: f32,
    model: u8,
    sample_rate: u32,
) -> Vec<DecodedMessage> {
    use mfsk_core::engine::dsp::resample::resample_f32_to_12k_f32;
    use mfsk_core::fec::qra::FadingModel;
    let audio = resample_f32_to_12k_f32(samples, sample_rate);
    let params = q65_wav_search_params();
    let nominal_mid = q65_slot_midpoint_samples(submode);
    let fading = match model {
        1 => FadingModel::Lorentzian,
        _ => FadingModel::Gaussian,
    };
    macro_rules! scan_body {
        ($p:ty) => {
            mfsk_core::q65::DecodeRequest::<$p>::new(&audio, 12_000, nominal_mid, params)
                .fading(fading, b90_ts)
                .decode()
        };
    }
    let decodes = dispatch_q65_submode!(submode, scan_body);
    q65_decodes_to_messages(decodes)
}

/// f32 → i16 wrapper for the fast-fading variant. `b90_ts` and
/// `model` semantics identical to [`decode_q65_wav_fading_f32`].
#[wasm_bindgen]
pub fn decode_q65_wav_fading(
    samples: &[i16],
    submode: u8,
    b90_ts: f32,
    model: u8,
    sample_rate: u32,
) -> Vec<DecodedMessage> {
    use mfsk_core::engine::dsp::resample::resample_i16_to_12k_f32;
    use mfsk_core::fec::qra::FadingModel;
    let audio = resample_i16_to_12k_f32(samples, sample_rate);
    let params = q65_wav_search_params();
    let nominal_mid = q65_slot_midpoint_samples(submode);
    let fading = match model {
        1 => FadingModel::Lorentzian,
        _ => FadingModel::Gaussian,
    };
    macro_rules! scan_body {
        ($p:ty) => {
            mfsk_core::q65::DecodeRequest::<$p>::new(&audio, 12_000, nominal_mid, params)
                .fading(fading, b90_ts)
                .decode()
        };
    }
    let decodes = dispatch_q65_submode!(submode, scan_body);
    q65_decodes_to_messages(decodes)
}

/// Encode a standard Q65 message (`<call1> <call2> <grid_or_report>`)
/// at the requested sub-mode + audio centre frequency. Returns 12 kHz
/// f32 PCM at amplitude 0.3.
#[wasm_bindgen]
pub fn encode_q65(
    call1: &str,
    call2: &str,
    grid_or_report: &str,
    freq_hz: f32,
    submode: u8,
) -> Result<Vec<f32>, JsValue> {
    let result = match submode {
        0 => mfsk_core::q65::synthesize_standard_for::<mfsk_core::q65::Q65a30>(
            call1, call2, grid_or_report, 12_000, freq_hz, 0.3,
        ),
        1 => mfsk_core::q65::synthesize_standard_for::<mfsk_core::q65::Q65a60>(
            call1, call2, grid_or_report, 12_000, freq_hz, 0.3,
        ),
        2 => mfsk_core::q65::synthesize_standard_for::<mfsk_core::q65::Q65b60>(
            call1, call2, grid_or_report, 12_000, freq_hz, 0.3,
        ),
        3 => mfsk_core::q65::synthesize_standard_for::<mfsk_core::q65::Q65c60>(
            call1, call2, grid_or_report, 12_000, freq_hz, 0.3,
        ),
        4 => mfsk_core::q65::synthesize_standard_for::<mfsk_core::q65::Q65d60>(
            call1, call2, grid_or_report, 12_000, freq_hz, 0.3,
        ),
        5 => mfsk_core::q65::synthesize_standard_for::<mfsk_core::q65::Q65e60>(
            call1, call2, grid_or_report, 12_000, freq_hz, 0.3,
        ),
        _ => return Err(JsValue::from_str("Invalid Q65 sub-mode (expected 0..=5)")),
    };
    result.ok_or_else(|| JsValue::from_str("Q65 message pack failed (bad callsign / grid / report)"))
}
