use wasm_bindgen::prelude::*;
use mfsk_core::decoder::{
    default_params, Decoder, Depth, Fst4Extras, Fst4Strategy, Ft4Extras, Ft4Strategy, Ft8Extras,
    Ft8Strategy, Q65Extras, SearchTuning, SlotInput, Sniper, Tuning,
};
use mfsk_core::engine::equalize::EqMode;
use mfsk_core::engine::pipeline::DecodeStrictness;
use mfsk_core::engine::sync::bootstrap_dt_median;
use mfsk_core::engine::tx::{message_to_tones, synthesize, FskWaveform};
use mfsk_core::ft8::decode_block::{coarse_sync, compute_spectrogram};
use mfsk_core::ft8::resample::{resample_f32_to_12k, resample_to_12k};
use mfsk_core::msg::ApHint;
use mfsk_core::{Ft4, Ft8, Mode};
use js_sys::Function;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::Mutex;

// mfsk-core 0.13 replaced the per-protocol `DecodeRequest` builders with one
// persistent `Decoder<P>` per mode. Everything the old thread_locals did by
// hand now lives inside it: the callsign hash table (learned from every
// decode, so `register_callsigns` is gone), and the plausibility filter
// (`MessageFilter::Default`). There is no staged/early-decode entry point and
// no `known`/`fft_cache` hand-off, so the FT8 Phase 1 / Phase 2 split below is
// rebuilt on `decode_with`'s per-row callback instead: Phase 1 is a cheap
// single pass whose rows stream out as they are found; Phase 2 is the SIC
// decode, which re-finds those strong rows and streams only the new ones.
//
// `osd: true` is set explicitly everywhere — disabling it cost real recall in
// every scenario measured (see the 2026-07-26 depth-matrix notes in git
// history); device-class shedding should target the SIC strategy, not OSD.

thread_local! {
    static DEC8: RefCell<Decoder<Ft8>> = RefCell::new(Decoder::with_defaults());
    static DEC4: RefCell<Decoder<Ft4>> = RefCell::new(Decoder::with_defaults());
    /// Audio from Phase 1, consumed by Phase 2.
    static CACHED_AUDIO: RefCell<Option<Audio>> = RefCell::new(None);
    /// Message texts Phase 1 already delivered; Phase 2 skips them.
    static CACHED_PHASE1: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Absolute `Date.now()` deadline for the *next* decode only; see
    /// [`set_decode_budget_ms`].
    static DEADLINE_MS: Cell<Option<f64>> = Cell::new(None);
}

/// Give the next decode a wall-clock budget of `ms` milliseconds from now.
///
/// mfsk-core stops starting new candidates once its budget predicate returns
/// `false` (a candidate already running finishes), and keeps what it found.
/// The crate has no clock of its own on wasm32-unknown-unknown — `Instant`
/// is unimplemented there — so the clock is `Date.now()` here. Consumed by
/// the next decode and then cleared; call it again for each decode that
/// should be budgeted. `ms <= 0` means the budget is already spent: the
/// decode starts no candidate beyond what the engine does unconditionally.
#[wasm_bindgen]
pub fn set_decode_budget_ms(ms: f64) {
    DEADLINE_MS.with(|d| d.set(Some(js_sys::Date::now() + ms)));
}

/// Owned 12 kHz 16-bit audio (the f32 live path is quantised on entry, as it
/// always was, so both paths feed the decoder at the same level).
enum Audio {
    I16(Vec<i16>),
}

impl Audio {
    fn slot(&self) -> SlotInput<'_> {
        match self {
            Audio::I16(a) => SlotInput::i16(a),
        }
    }
}

fn audio_i16(samples: &[i16], sample_rate: u32) -> Audio {
    Audio::I16(if sample_rate != 12000 { resample_to_12k(samples, sample_rate) } else { samples.to_vec() })
}

/// f32 live path: resampled and quantised to i16 exactly as before.
fn audio_f32(samples: &[f32], sample_rate: u32) -> Audio {
    Audio::I16(resample_f32_to_12k(samples, sample_rate))
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

fn row_to_decoded<R>(r: &mfsk_core::decoder::Row<R>) -> DecodedMessage {
    DecodedMessage {
        freq_hz: r.decoded.freq_hz,
        dt_sec: r.decoded.dt_sec,
        snr_db: r.decoded.snr_db,
        hard_errors: r.detail.hard_errors,
        pass: r.detail.pass,
        message: r.decoded.text.clone(),
    }
}

fn to_strictness(level: u8) -> DecodeStrictness {
    match level {
        0 => DecodeStrictness::Strict,
        2 => DecodeStrictness::Deep,
        _ => DecodeStrictness::Normal,
    }
}

/// FT4 (3-tier): Fast uses a light `SicRounds(2)`; Normal/Deep the full 3.
fn ft4_rounds(profile: u8) -> usize {
    if profile == 0 { 2 } else { 3 }
}

/// FT8 Phase 2 strength: Normal gets a single light SIC round (91% of Deep's
/// recall for ~half the time on qso3_busy.wav); Deep keeps `SicEarly`. Fast is
/// Phase 1 alone — `app.js` skips the Phase 2 call for profile 0.
fn ft8_sic(profile: u8) -> Ft8Strategy {
    if profile == 1 { Ft8Strategy::SicRounds(1) } else { Ft8Strategy::SicEarly }
}

/// Bridges a plain synchronous `Fn(&Row) + Sync` row callback across the
/// wasm-bindgen boundary to a JS `Function`. `js_sys::Function` is `!Sync`,
/// but this build is single-threaded wasm32 (`parallel` off, no atomics), so
/// concurrent access is physically impossible and the `unsafe impl Sync` is
/// sound. Revisit if this crate ever gains real wasm threading.
struct JsCallbackSync(Function);
unsafe impl Sync for JsCallbackSync {}

/// Run one decode on `dec`, streaming each row not in `skip` to `cb` (if any)
/// as it is found, and return the rows not in `skip`. Rows within one call are
/// also de-duplicated by text, so a candidate found twice is delivered once.
fn run<P: mfsk_core::decoder::Decodable>(
    dec: &mut Decoder<P>,
    audio: &Audio,
    skip: &HashSet<String>,
    cb: Option<&JsCallbackSync>,
) -> Vec<DecodedMessage> {
    run_slot(dec, &audio.slot(), skip, cb)
}

fn run_slot<P: mfsk_core::decoder::Decodable>(
    dec: &mut Decoder<P>,
    slot: &SlotInput<'_>,
    skip: &HashSet<String>,
    cb: Option<&JsCallbackSync>,
) -> Vec<DecodedMessage> {
    let deadline = DEADLINE_MS.with(|d| d.take());
    let within_budget = move || deadline.is_none_or(|t| js_sys::Date::now() < t);
    let slot = match deadline {
        Some(_) => slot.budget(&within_budget),
        None => *slot,
    };
    let slot = &slot;
    let sent = Mutex::new(HashSet::<String>::new());
    let fresh = |text: &str| !skip.contains(text) && sent.lock().unwrap().insert(text.to_string());
    let result = match cb {
        Some(cb) => dec.decode_with(slot, &|row| {
            if fresh(&row.decoded.text) {
                let _ = cb.0.call1(&JsValue::NULL, &JsValue::from(row_to_decoded(row)));
            }
        }),
        None => dec.decode(slot),
    };
    let mut seen = HashSet::new();
    result
        .rows
        .iter()
        .filter(|r| !skip.contains(&r.decoded.text) && seen.insert(r.decoded.text.clone()))
        .map(row_to_decoded)
        .collect()
}

// ──────────────────────────────────────────────────────────────────────────
// FT8
// ──────────────────────────────────────────────────────────────────────────

struct Ft8Run {
    band: (f32, f32),
    sync_min: f32,
    max_cand: usize,
    strategy: Ft8Strategy,
    strictness: DecodeStrictness,
    eq: EqMode,
    ap_hint: Option<ApHint>,
    /// `Some(target_hz)`: narrow sniper search around the target.
    sniper: Option<f32>,
}

impl Ft8Run {
    fn wide(sync_min: f32, strategy: Ft8Strategy, strictness: DecodeStrictness) -> Self {
        Self {
            band: (100.0, 3000.0),
            sync_min,
            max_cand: 200,
            strategy,
            strictness,
            eq: EqMode::Off,
            ap_hint: None,
            sniper: None,
        }
    }
}

fn ft8_decode(audio: &Audio, cfg: Ft8Run, skip: &HashSet<String>, cb: Option<&JsCallbackSync>) -> Vec<DecodedMessage> {
    DEC8.with(|d| {
        let mut d = d.borrow_mut();
        let mut p = default_params(Mode::Ft8).band(cfg.band.0, cfg.band.1).depth(Depth::Deep);
        if let Some(t) = cfg.sniper {
            p = p.rx_freq(t);
        }
        *d.params_mut() = p;
        *d.extras_mut() = Ft8Extras {
            tuning: Tuning {
                sync_min: Some(cfg.sync_min),
                max_cand: Some(cfg.max_cand),
                osd: Some(true),
                strictness: Some(cfg.strictness),
                strategy: Some(cfg.strategy),
            },
            ap_hint: cfg.ap_hint,
            eq: cfg.eq,
            sniper: cfg.sniper.map(|_| Sniper::default()),
            ..Ft8Extras::default()
        };
        run(&mut d, audio, skip, cb)
    })
}

fn no_skip() -> HashSet<String> {
    HashSet::new()
}

/// Build an ApHint from the supplied AP target fields.
///
/// | callsign | grid | mycall | Hint built                            |
/// |----------|------|--------|---------------------------------------|
/// | empty    | empty| any    | None                                  |
/// | empty    | set  | any    | grid only                             |
/// | set      | any  | empty  | CQ + call2 [+ grid] (Watch phase)     |
/// | set      | any  | set    | mycall + call2       (Call phase)      |
fn build_ap_hint(callsign: &str, grid: &str, mycall: &str) -> Option<ApHint> {
    if callsign.is_empty() && grid.is_empty() {
        None
    } else if callsign.is_empty() {
        Some(ApHint::new().with_grid(grid))
    } else if mycall.is_empty() {
        let mut h = ApHint::new().with_call1("CQ").with_call2(callsign);
        if !grid.is_empty() { h = h.with_grid(grid); }
        Some(h)
    } else {
        // Call phase: grid ignored — its bits overlap the report field.
        Some(ApHint::new().with_call1(mycall).with_call2(callsign))
    }
}

/// Decode a 15-second FT8 audio frame (wide-band scan, single pass).
///
/// `sample_rate` — input PCM sample rate in Hz; non-12 000 Hz is resampled.
#[wasm_bindgen]
pub fn decode_wav(samples: &[i16], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let a = audio_i16(samples, sample_rate);
    ft8_decode(&a, Ft8Run::wide(1.5, Ft8Strategy::SinglePass, to_strictness(strictness)), &no_skip(), None)
}

/// f32 variant of [`decode_wav`].
#[wasm_bindgen]
pub fn decode_wav_f32(samples: &[f32], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let a = audio_f32(samples, sample_rate);
    ft8_decode(&a, Ft8Run::wide(1.5, Ft8Strategy::SinglePass, to_strictness(strictness)), &no_skip(), None)
}

/// Decode with iterative signal subtraction.
#[wasm_bindgen]
pub fn decode_wav_subtract(samples: &[i16], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let a = audio_i16(samples, sample_rate);
    ft8_decode(&a, Ft8Run::wide(1.0, Ft8Strategy::SicEarly, to_strictness(strictness)), &no_skip(), None)
}

/// f32 variant of [`decode_wav_subtract`].
#[wasm_bindgen]
pub fn decode_wav_subtract_f32(samples: &[f32], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    let a = audio_f32(samples, sample_rate);
    ft8_decode(&a, Ft8Run::wide(1.0, Ft8Strategy::SicEarly, to_strictness(strictness)), &no_skip(), None)
}

/// Narrow-band (±250 Hz around `target_freq`) sniper decode with SIC and the
/// adaptive equalizer, plus multi-pass AP from the supplied hint fields.
///
/// Pass `mycall = ""` for Watch phase (CQ-style hint + grid), or the own call
/// for Call phase (QSO hint, grid ignored).
fn sniper_decode(audio: &Audio, target_freq: f32, eq_on: bool, ap: Option<ApHint>) -> Vec<DecodedMessage> {
    ft8_decode(
        audio,
        Ft8Run {
            band: ((target_freq - 250.0).max(100.0), (target_freq + 250.0).min(5900.0)),
            sync_min: 0.8,
            max_cand: 20,
            strategy: Ft8Strategy::SicEarly,
            strictness: DecodeStrictness::Normal,
            eq: if eq_on { EqMode::Local } else { EqMode::Off },
            ap_hint: ap,
            sniper: Some(target_freq),
        },
        &no_skip(),
        None,
    )
}

#[wasm_bindgen]
pub fn decode_sniper(samples: &[i16], target_freq: f32, callsign: &str, grid: &str, mycall: &str, eq_on: bool, sample_rate: u32) -> Vec<DecodedMessage> {
    let a = audio_i16(samples, sample_rate);
    sniper_decode(&a, target_freq, eq_on, build_ap_hint(callsign, grid, mycall))
}

/// f32 variant of [`decode_sniper`].
#[wasm_bindgen]
pub fn decode_sniper_f32(samples: &[f32], target_freq: f32, callsign: &str, grid: &str, mycall: &str, eq_on: bool, sample_rate: u32) -> Vec<DecodedMessage> {
    let a = audio_f32(samples, sample_rate);
    sniper_decode(&a, target_freq, eq_on, build_ap_hint(callsign, grid, mycall))
}

/// Cold-start DT estimate from `coarse_sync` candidates: the DT median of the
/// top-5 candidates by score. Useful for seeding the JS-side period manager
/// when the device clock is skewed >2 s from UTC.
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

#[wasm_bindgen]
pub fn encode_ft8(call1: &str, call2: &str, report: &str, freq_hz: f32) -> Result<Vec<f32>, JsValue> {
    let msg77 = mfsk_core::msg::wsjt77::pack77(call1, call2, report)
        .ok_or_else(|| JsValue::from_str("Failed to pack message"))?;
    Ok(encode_wave::<Ft8>(&msg77, freq_hz))
}

/// Encode a free-text FT8 message (Type 0, n3=0) as audio samples.
///
/// `text` — up to 13 characters from the FT8 free-text alphabet.
#[wasm_bindgen]
pub fn encode_free_text(text: &str, freq_hz: f32) -> Result<Vec<f32>, JsValue> {
    let msg77 = mfsk_core::msg::wsjt77::pack77_free_text(text)
        .ok_or_else(|| JsValue::from_str("Invalid free text (max 13 chars, 0-9 A-Z +-./?)"))?;
    Ok(encode_wave::<Ft8>(&msg77, freq_hz))
}

/// 12 kHz f32 PCM at amplitude 1.0 for `P`'s own waveform (GFSK shaping per
/// mode, including each FST4 sub-mode's).
fn encode_wave<P: FskWaveform + mfsk_core::engine::Protocol>(msg77: &[u8; 77], freq_hz: f32) -> Vec<f32> {
    let tones = message_to_tones::<P>(msg77);
    synthesize::<P>(&tones, 12_000, freq_hz, 1.0)
}

// ──────────────────────────────────────────────────────────────────────────
// Pipelined decode: Phase 1 (fast) + Phase 2 (SIC)
//
// Phase 1 is a single pass; its rows stream out as found and their texts are
// remembered. Phase 2 re-decodes the same audio with SIC and delivers only
// what Phase 1 did not. (mfsk-core 0.13 has no staged API to resume from.)
// ──────────────────────────────────────────────────────────────────────────

fn phase1(audio: Audio, cb: Option<&JsCallbackSync>) -> Vec<DecodedMessage> {
    let out = ft8_decode(&audio, Ft8Run::wide(1.5, Ft8Strategy::SinglePass, DecodeStrictness::Normal), &no_skip(), cb);
    CACHED_PHASE1.with(|p| *p.borrow_mut() = out.iter().map(|m| m.message()).collect());
    CACHED_AUDIO.with(|a| *a.borrow_mut() = Some(audio));
    out
}

fn phase2(profile: u8, cb: Option<&JsCallbackSync>) -> Vec<DecodedMessage> {
    let audio = CACHED_AUDIO.with(|a| a.borrow_mut().take()).expect("decode_phase1 must run first");
    let known = CACHED_PHASE1.with(|p| std::mem::take(&mut *p.borrow_mut()));
    ft8_decode(&audio, Ft8Run::wide(1.0, ft8_sic(profile), to_strictness(profile)), &known, cb)
}

/// Phase 1 decode (i16): fast single-pass decode. Caches the audio for
/// [`decode_phase2`]. Panics in Phase 2 if this was not called first.
#[wasm_bindgen]
pub fn decode_phase1(samples: &[i16], sample_rate: u32) -> Vec<DecodedMessage> {
    phase1(audio_i16(samples, sample_rate), None)
}

/// Phase 2 decode: SIC, strength picked by the GUI decode-profile level
/// (see `ft8_sic`). Returns only messages Phase 1 did not find.
#[wasm_bindgen]
pub fn decode_phase2(profile: u8) -> Vec<DecodedMessage> {
    phase2(profile, None)
}

/// f32 variant of [`decode_phase1`] for the live AudioWorklet path.
#[wasm_bindgen]
pub fn decode_phase1_f32(samples: &[f32], sample_rate: u32) -> Vec<DecodedMessage> {
    phase1(audio_f32(samples, sample_rate), None)
}

/// f32 variant of [`decode_phase2`].
#[wasm_bindgen]
pub fn decode_phase2_f32(profile: u8) -> Vec<DecodedMessage> {
    phase2(profile, None)
}

/// Streaming Phase 1 (i16): `on_result(msg)` once per accepted candidate as
/// found, in addition to returning the full batch.
#[wasm_bindgen]
pub fn decode_phase1_streaming(samples: &[i16], sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    phase1(audio_i16(samples, sample_rate), Some(&JsCallbackSync(on_result)))
}

/// Streaming Phase 2: `on_result(msg)` once per newly found SIC candidate.
#[wasm_bindgen]
pub fn decode_phase2_streaming(profile: u8, on_result: Function) -> Vec<DecodedMessage> {
    phase2(profile, Some(&JsCallbackSync(on_result)))
}

/// Streaming Phase 1 (f32), for the live AudioWorklet path.
#[wasm_bindgen]
pub fn decode_phase1_streaming_f32(samples: &[f32], sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    phase1(audio_f32(samples, sample_rate), Some(&JsCallbackSync(on_result)))
}

/// Streaming Phase 2 (f32 twin of [`decode_phase2_streaming`]).
#[wasm_bindgen]
pub fn decode_phase2_streaming_f32(profile: u8, on_result: Function) -> Vec<DecodedMessage> {
    phase2(profile, Some(&JsCallbackSync(on_result)))
}

// ──────────────────────────────────────────────────────────────────────────
// FT4 / FST4 — share the 77-bit WSJT message format and the frame decoder.
// ──────────────────────────────────────────────────────────────────────────

fn ft4_decode(
    audio: &Audio,
    band: (f32, f32),
    rx_freq: Option<f32>,
    max_cand: usize,
    strategy: Ft4Strategy,
    strictness: DecodeStrictness,
    eq: EqMode,
    ap_hint: Option<ApHint>,
    cb: Option<&JsCallbackSync>,
) -> Vec<DecodedMessage> {
    DEC4.with(|d| {
        let mut d = d.borrow_mut();
        let mut p = default_params(Mode::Ft4).band(band.0, band.1).depth(Depth::Deep);
        if let Some(f) = rx_freq {
            p = p.rx_freq(f);
        }
        *d.params_mut() = p;
        *d.extras_mut() = Ft4Extras {
            tuning: Tuning {
                sync_min: Some(1.2),
                max_cand: Some(max_cand),
                osd: Some(true),
                strictness: Some(strictness),
                strategy: Some(strategy),
            },
            ap_hint,
            eq,
            ..Ft4Extras::default()
        };
        run(&mut d, audio, &no_skip(), cb)
    })
}

fn ft4_wide(audio: &Audio, strategy: Ft4Strategy, strictness: DecodeStrictness, cb: Option<&JsCallbackSync>) -> Vec<DecodedMessage> {
    ft4_decode(audio, (300.0, 2700.0), None, 50, strategy, strictness, EqMode::Off, None, cb)
}

/// Decode a 7.5-second FT4 slot (wide-band scan, single pass).
#[wasm_bindgen]
pub fn decode_ft4_wav(samples: &[i16], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    ft4_wide(&audio_i16(samples, sample_rate), Ft4Strategy::SinglePass, to_strictness(strictness), None)
}

/// f32 variant of [`decode_ft4_wav`].
#[wasm_bindgen]
pub fn decode_ft4_wav_f32(samples: &[f32], strictness: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    ft4_wide(&audio_f32(samples, sample_rate), Ft4Strategy::SinglePass, to_strictness(strictness), None)
}

/// FT4 multi-pass subtract decode (SIC) for crowded slots. `profile`
/// (0=Fast/1=Normal/2=Deep) picks both strictness and SIC round count.
#[wasm_bindgen]
pub fn decode_ft4_wav_subtract(samples: &[i16], profile: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    ft4_wide(&audio_i16(samples, sample_rate), Ft4Strategy::SicRounds(ft4_rounds(profile)), to_strictness(profile), None)
}

/// f32 variant of [`decode_ft4_wav_subtract`].
#[wasm_bindgen]
pub fn decode_ft4_wav_subtract_f32(samples: &[f32], profile: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    ft4_wide(&audio_f32(samples, sample_rate), Ft4Strategy::SicRounds(ft4_rounds(profile)), to_strictness(profile), None)
}

/// Streaming sibling of [`decode_ft4_wav_subtract`]: `on_result(msg)` once per
/// accepted candidate as it is found.
#[wasm_bindgen]
pub fn decode_ft4_wav_subtract_streaming(samples: &[i16], profile: u8, sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    ft4_wide(&audio_i16(samples, sample_rate), Ft4Strategy::SicRounds(ft4_rounds(profile)), to_strictness(profile), Some(&JsCallbackSync(on_result)))
}

/// f32 variant of [`decode_ft4_wav_subtract_streaming`].
#[wasm_bindgen]
pub fn decode_ft4_wav_subtract_streaming_f32(samples: &[f32], profile: u8, sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    ft4_wide(&audio_f32(samples, sample_rate), Ft4Strategy::SicRounds(ft4_rounds(profile)), to_strictness(profile), Some(&JsCallbackSync(on_result)))
}

fn ft4_sniper(audio: &Audio, target_freq: f32, callsign: &str, mycall: &str, eq_on: bool) -> Vec<DecodedMessage> {
    let ap = if callsign.is_empty() {
        None
    } else if mycall.is_empty() {
        Some(ApHint::new().with_call1("CQ").with_call2(callsign))
    } else {
        Some(ApHint::new().with_call1(mycall).with_call2(callsign))
    };
    ft4_decode(
        audio,
        ((target_freq - 250.0).max(100.0), (target_freq + 250.0).min(5900.0)),
        Some(target_freq),
        15,
        Ft4Strategy::SicRounds(3),
        DecodeStrictness::Normal,
        if eq_on { EqMode::Local } else { EqMode::Off },
        ap,
        None,
    )
}

/// FT4 sniper-mode decode at a target frequency with optional AP hints.
#[wasm_bindgen]
pub fn decode_ft4_sniper(samples: &[i16], target_freq: f32, callsign: &str, mycall: &str, eq_on: bool, sample_rate: u32) -> Vec<DecodedMessage> {
    ft4_sniper(&audio_i16(samples, sample_rate), target_freq, callsign, mycall, eq_on)
}

/// f32 variant of [`decode_ft4_sniper`].
#[wasm_bindgen]
pub fn decode_ft4_sniper_f32(samples: &[f32], target_freq: f32, callsign: &str, mycall: &str, eq_on: bool, sample_rate: u32) -> Vec<DecodedMessage> {
    ft4_sniper(&audio_f32(samples, sample_rate), target_freq, callsign, mycall, eq_on)
}

/// Encode an FT4 standard message (CALL1 CALL2 GRID/REPORT) as 12 kHz PCM.
#[wasm_bindgen]
pub fn encode_ft4(call1: &str, call2: &str, report: &str, freq_hz: f32) -> Result<Vec<f32>, JsValue> {
    let msg77 = mfsk_core::msg::wsjt77::pack77(call1, call2, report).ok_or_else(|| JsValue::from_str("Failed to pack message"))?;
    Ok(encode_wave::<Ft4>(&msg77, freq_hz))
}

/// Encode a free-text FT4 message (up to 13 chars from the FT8 alphabet).
#[wasm_bindgen]
pub fn encode_ft4_free_text(text: &str, freq_hz: f32) -> Result<Vec<f32>, JsValue> {
    let msg77 = mfsk_core::msg::wsjt77::pack77_free_text(text).ok_or_else(|| JsValue::from_str("Invalid free text"))?;
    Ok(encode_wave::<Ft4>(&msg77, freq_hz))
}

// ───────────────────────────────────────────────────────────────────────
// FST4 — five wired sub-modes (FST4-15 / -30 / -60 / -120 / -300)
//
//   0 = FST4-15  (15 s)    1 = FST4-30  (30 s)    2 = FST4-60  (60 s)
//   3 = FST4-120 (120 s)   4 = FST4-300 (300 s)
//
// FST4 slots are decoded one-shot with a fresh `Decoder` (nothing is carried
// between slots), plain wide-band scan only. Frame coarse-sync finds the time
// offset itself, so no nominal-start hint is needed (unlike Q65).
// ───────────────────────────────────────────────────────────────────────

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

fn fst4_decode(audio: &Audio, submode: u8, profile: u8, cb: Option<&JsCallbackSync>) -> Vec<DecodedMessage> {
    macro_rules! body {
        ($p:ty) => {{
            let mut d = Decoder::<$p>::new(
                default_params(<$p as mfsk_core::decoder::Decodable>::MODE)
                    .band(FST4_FLOW, FST4_FHIGH)
                    .depth(Depth::Deep),
            );
            *d.extras_mut() = Fst4Extras {
                tuning: Tuning {
                    sync_min: Some(FST4_SYNC_MIN),
                    max_cand: Some(FST4_MAX_CAND),
                    osd: Some(true),
                    strictness: Some(to_strictness(profile)),
                    strategy: Some(Fst4Strategy::SinglePass),
                },
                ..Fst4Extras::default()
            };
            run(&mut d, audio, &no_skip(), cb)
        }};
    }
    dispatch_fst4_submode!(submode, body)
}

/// Decode an FST4 slot (wide-band scan). `submode` 0..=4 picks the T/R
/// period; `profile` (0=Fast/1=Normal/2=Deep) maps to `DecodeStrictness`.
#[wasm_bindgen]
pub fn decode_fst4_wav(samples: &[i16], submode: u8, profile: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    fst4_decode(&audio_i16(samples, sample_rate), submode, profile, None)
}

/// f32 variant of [`decode_fst4_wav`].
#[wasm_bindgen]
pub fn decode_fst4_wav_f32(samples: &[f32], submode: u8, profile: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    fst4_decode(&audio_f32(samples, sample_rate), submode, profile, None)
}

/// Streaming sibling of [`decode_fst4_wav`] — most valuable here, since FST4
/// slots run 15-300 s.
#[wasm_bindgen]
pub fn decode_fst4_wav_streaming(samples: &[i16], submode: u8, profile: u8, sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    fst4_decode(&audio_i16(samples, sample_rate), submode, profile, Some(&JsCallbackSync(on_result)))
}

/// f32 variant of [`decode_fst4_wav_streaming`].
#[wasm_bindgen]
pub fn decode_fst4_wav_streaming_f32(samples: &[f32], submode: u8, profile: u8, sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    fst4_decode(&audio_f32(samples, sample_rate), submode, profile, Some(&JsCallbackSync(on_result)))
}

/// Encode a standard FST4 message at the requested sub-mode + audio centre
/// frequency. `submode` 0..=4 picks the T/R period, which selects that
/// sub-mode's own GFSK pulse shaping; the 77-bit message packing is shared
/// with FT4/FT8. Returns 12 kHz f32 PCM at amplitude 1.0.
#[wasm_bindgen]
pub fn encode_fst4(call1: &str, call2: &str, report: &str, freq_hz: f32, submode: u8) -> Result<Vec<f32>, JsValue> {
    use mfsk_core::fst4::{Fst4s120, Fst4s15, Fst4s30, Fst4s300, Fst4s60};
    let msg77 = mfsk_core::msg::wsjt77::pack77(call1, call2, report).ok_or_else(|| JsValue::from_str("Failed to pack message"))?;
    Ok(match submode {
        0 => encode_wave::<Fst4s15>(&msg77, freq_hz),
        1 => encode_wave::<Fst4s30>(&msg77, freq_hz),
        2 => encode_wave::<Fst4s60>(&msg77, freq_hz),
        3 => encode_wave::<Fst4s120>(&msg77, freq_hz),
        4 => encode_wave::<Fst4s300>(&msg77, freq_hz),
        _ => return Err(JsValue::from_str("Invalid FST4 sub-mode (expected 0..=4)")),
    })
}

// ───────────────────────────────────────────────────────────────────────
// WSPR
// ───────────────────────────────────────────────────────────────────────

/// The decoder's `Decoded.freq_hz` for WSPR is the **centre** of the 4-tone
/// group (tone 0 + 1.5 × spacing), which is what wsprd reports and what the
/// operator dials — the correction this file used to apply by hand is now
/// done inside mfsk-core. `dt_sec` stays the absolute start offset in the
/// 12 kHz pipeline (`start_sample / 12000`), as before.
fn wspr_row(r: &mfsk_core::decoder::Row<mfsk_core::wspr::WsprResult>) -> DecodedMessage {
    DecodedMessage {
        freq_hz: r.decoded.freq_hz,
        dt_sec: r.native.start_sample as f32 / 12_000.0,
        snr_db: r.decoded.snr_db,
        hard_errors: 0,
        pass: 0,
        message: r.decoded.text.clone(),
    }
}

fn wspr_decode(audio: Vec<f32>, cb: Option<&JsCallbackSync>) -> Vec<DecodedMessage> {
    let mut d = Decoder::<mfsk_core::wspr::Wspr>::with_defaults();
    let slot = SlotInput::f32(&audio);
    let res = match cb {
        Some(cb) => d.decode_with(&slot, &|r| {
            let _ = cb.0.call1(&JsValue::NULL, &JsValue::from(wspr_row(r)));
        }),
        None => d.decode(&slot),
    };
    res.rows.iter().map(wspr_row).collect()
}

/// Decode a 120-s WSPR slot. Non-12 kHz input is auto-resampled.
#[wasm_bindgen]
pub fn decode_wspr_wav(samples: &[i16], sample_rate: u32) -> Vec<DecodedMessage> {
    wspr_decode(mfsk_core::engine::dsp::resample::resample_i16_to_12k_f32(samples, sample_rate), None)
}

/// f32 variant of [`decode_wspr_wav`].
#[wasm_bindgen]
pub fn decode_wspr_wav_f32(samples: &[f32], sample_rate: u32) -> Vec<DecodedMessage> {
    wspr_decode(mfsk_core::engine::dsp::resample::resample_f32_to_12k_f32(samples, sample_rate), None)
}

/// Streaming sibling of [`decode_wspr_wav`]: `on_result(msg)` per accepted candidate.
#[wasm_bindgen]
pub fn decode_wspr_wav_streaming(samples: &[i16], sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    wspr_decode(mfsk_core::engine::dsp::resample::resample_i16_to_12k_f32(samples, sample_rate), Some(&JsCallbackSync(on_result)))
}

/// f32 variant of [`decode_wspr_wav_streaming`].
#[wasm_bindgen]
pub fn decode_wspr_wav_streaming_f32(samples: &[f32], sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    wspr_decode(mfsk_core::engine::dsp::resample::resample_f32_to_12k_f32(samples, sample_rate), Some(&JsCallbackSync(on_result)))
}

/// Encode a Type-1 WSPR beacon transmission — callsign + 4-character
/// grid + power in dBm — as 12 kHz f32 PCM at amplitude 1.0.
/// 162 symbols x 8192 samples = 1 327 104 samples, 110.6 s.
///
/// Two things here were wrong while this function had no caller, and
/// both would have been silent on the air rather than obvious:
///
/// **`freq_hz` is the centre of the 4-tone group**, matching what the
/// operator dials and what WSJT-X's Tx-frequency spin box means.
/// mfsk-core's synthesiser takes tone 0, so the 1.5 x spacing offset is
/// applied here exactly as `mainwindow.cpp` does it:
///
/// ```text
/// Q_EMIT sendMessage (m_mode, NUM_WSPR_SYMBOLS, 8192.0,
///                     ui->TxFreqSpinBox->value() - 1.5 * 12000 / 8192, ...
/// ```
///
/// This previously passed `freq_hz` straight through as tone 0, putting
/// the signal 2.2 Hz above where it was dialled. Invisible on a
/// waterfall; not invisible in a mode whose entire occupied bandwidth
/// is 6 Hz inside a 200 Hz sub-band.
///
/// **Amplitude is 1.0**, like `encode_ft8`/`encode_ft4`/`encode_q65`/
/// `encode_fst4`. It was 0.3, which would have put WSPR 10.5 dB below
/// every other mode at the same TX-gain slider position, and made
/// `AudioOutput.peakLevel`'s pre-gain meter read 30 % at full drive.
/// Level belongs to the slider, not to the encoder.
///
/// No GFSK symbol shaping, deliberately — WSJT-X passes a positive
/// `toneSpacing` for WSPR, selecting `Modulator::modulate`'s plain
/// CPFSK branch rather than the pre-computed filtered-waveform branch
/// FT8/FT4/FST4 use. See `app.js`'s `encodeTx` for the full note. The
/// burst envelope *is* ramped, by mfsk-core 47f0e63
/// (`engine::dsp::envelope`, issue #259); without it the 110.6 s burst
/// would begin and end on a step discontinuity.
///
/// Errors if the arguments cannot fit the Type-1 layout. Note WSPR
/// takes a **4-character** grid — callers must truncate 6-character
/// locators.
#[wasm_bindgen]
pub fn encode_wspr(
    callsign: &str,
    grid: &str,
    power_dbm: i32,
    freq_hz: f32,
) -> Result<Vec<f32>, JsValue> {
    use mfsk_core::engine::ModulationParams;
    let spacing = <mfsk_core::wspr::Wspr as ModulationParams>::TONE_SPACING_HZ;
    let tone0 = freq_hz - 1.5 * spacing;
    mfsk_core::wspr::synthesize_type1(callsign, grid, power_dbm, 12_000, tone0, 1.0)
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
// `dt_sec = start_sample / 12_000` and `hard_errors = QRA BP iterations`.
//
// SNR is real (`Q65Result.snr_db`, WSJT-X 2500 Hz reference convention) and is
// *transmitted*: `setRxSnr` in app.js feeds qso.js's `_autoReport`, so a Q65
// decode's SNR becomes the signal report this station sends. It is the one
// protocol whose SNR this project has never verified locally.
// ───────────────────────────────────────────────────────────────────────

fn q65_row(r: &mfsk_core::decoder::Row<mfsk_core::q65::Q65Result>) -> DecodedMessage {
    DecodedMessage {
        freq_hz: r.native.freq_hz,
        dt_sec: r.native.start_sample as f32 / 12_000.0,
        snr_db: r.native.snr_db,
        hard_errors: r.native.iterations,
        pass: 0,
        message: r.native.message.clone(),
    }
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

/// Slot length in seconds per sub-mode index: 0 = Q65-30A, 1..=5 = Q65-60A‥E.
fn q65_slot_secs(submode: u8) -> f32 {
    match submode {
        0 => 30.0,
        _ => 60.0,
    }
}

/// Wide-tolerance search for offline WAV decode. The decoder's default window
/// is the live-audio operating point (a few seconds around the nominal slot
/// start) and returns nothing on a WAV-drop where the signal can begin
/// anywhere in the slot. Open the window to the whole slot and lower the score
/// threshold so weak ionoscatter / EME signals reach the BP / fading metric.
fn q65_wav_tuning(submode: u8) -> SearchTuning {
    SearchTuning {
        time_tolerance_early_sec: Some(q65_slot_secs(submode) / 2.0),
        time_tolerance_late_sec: Some(q65_slot_secs(submode)),
        score_threshold: Some(0.05),
        max_candidates: Some(32),
    }
}

fn q65_decode(
    audio: Vec<f32>,
    submode: u8,
    fading: Option<(mfsk_core::fec::qra::FadingModel, f32)>,
    cb: Option<&JsCallbackSync>,
) -> Vec<DecodedMessage> {
    macro_rules! body {
        ($p:ty) => {{
            let mut d = Decoder::<$p>::new(
                default_params(<$p as mfsk_core::decoder::Decodable>::MODE).band(200.0, 3_000.0),
            );
            *d.extras_mut() = Q65Extras { search: q65_wav_tuning(submode), fading, ..Q65Extras::default() };
            let slot = SlotInput::f32(&audio);
            let res = match cb {
                Some(cb) => d.decode_with(&slot, &|r| {
                    let _ = cb.0.call1(&JsValue::NULL, &JsValue::from(q65_row(r)));
                }),
                None => d.decode(&slot),
            };
            res.rows.iter().map(q65_row).collect()
        }};
    }
    dispatch_q65_submode!(submode, body)
}

fn fading_of(b90_ts: f32, model: u8) -> Option<(mfsk_core::fec::qra::FadingModel, f32)> {
    use mfsk_core::fec::qra::FadingModel;
    Some((if model == 1 { FadingModel::Lorentzian } else { FadingModel::Gaussian }, b90_ts))
}

/// Plain Q65 BP decode (basic AWGN strategy). f32 audio.
#[wasm_bindgen]
pub fn decode_q65_wav_f32(samples: &[f32], submode: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    q65_decode(mfsk_core::engine::dsp::resample::resample_f32_to_12k_f32(samples, sample_rate), submode, None, None)
}

/// Plain Q65 BP decode. i16 audio variant.
#[wasm_bindgen]
pub fn decode_q65_wav(samples: &[i16], submode: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    q65_decode(mfsk_core::engine::dsp::resample::resample_i16_to_12k_f32(samples, sample_rate), submode, None, None)
}

/// Q65 fast-fading metric decode (high-Doppler EME).
///
/// `b90_ts` is the spread-bandwidth × symbol-period dimensionless product.
/// Calibrated test values: 3 (light), 8 (moderate), 15 (heavy / 10+ GHz EME).
/// `model`: 0 = Gaussian, 1 = Lorentzian.
#[wasm_bindgen]
pub fn decode_q65_wav_fading_f32(samples: &[f32], submode: u8, b90_ts: f32, model: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    q65_decode(mfsk_core::engine::dsp::resample::resample_f32_to_12k_f32(samples, sample_rate), submode, fading_of(b90_ts, model), None)
}

/// i16 variant of [`decode_q65_wav_fading_f32`].
#[wasm_bindgen]
pub fn decode_q65_wav_fading(samples: &[i16], submode: u8, b90_ts: f32, model: u8, sample_rate: u32) -> Vec<DecodedMessage> {
    q65_decode(mfsk_core::engine::dsp::resample::resample_i16_to_12k_f32(samples, sample_rate), submode, fading_of(b90_ts, model), None)
}

/// Streaming sibling of [`decode_q65_wav`]: `on_result(msg)` per accepted candidate.
#[wasm_bindgen]
pub fn decode_q65_wav_streaming(samples: &[i16], submode: u8, sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    q65_decode(mfsk_core::engine::dsp::resample::resample_i16_to_12k_f32(samples, sample_rate), submode, None, Some(&JsCallbackSync(on_result)))
}

/// f32 variant of [`decode_q65_wav_streaming`].
#[wasm_bindgen]
pub fn decode_q65_wav_streaming_f32(samples: &[f32], submode: u8, sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    q65_decode(mfsk_core::engine::dsp::resample::resample_f32_to_12k_f32(samples, sample_rate), submode, None, Some(&JsCallbackSync(on_result)))
}

/// Streaming sibling of [`decode_q65_wav_fading`].
#[wasm_bindgen]
pub fn decode_q65_wav_fading_streaming(samples: &[i16], submode: u8, b90_ts: f32, model: u8, sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    q65_decode(mfsk_core::engine::dsp::resample::resample_i16_to_12k_f32(samples, sample_rate), submode, fading_of(b90_ts, model), Some(&JsCallbackSync(on_result)))
}

/// f32 variant of [`decode_q65_wav_fading_streaming`].
#[wasm_bindgen]
pub fn decode_q65_wav_fading_streaming_f32(samples: &[f32], submode: u8, b90_ts: f32, model: u8, sample_rate: u32, on_result: Function) -> Vec<DecodedMessage> {
    q65_decode(mfsk_core::engine::dsp::resample::resample_f32_to_12k_f32(samples, sample_rate), submode, fading_of(b90_ts, model), Some(&JsCallbackSync(on_result)))
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
