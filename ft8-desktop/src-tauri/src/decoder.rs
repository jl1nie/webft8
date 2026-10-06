use mfsk_core::decoder::{default_params, Decoder, Depth, Ft8Extras, Ft8Strategy, Row, SlotInput, Sniper, Tuning};
use mfsk_core::engine::equalize::EqMode;
use mfsk_core::engine::pipeline::DecodeStrictness;
use mfsk_core::engine::tx::{message_to_tones, synthesize};
use mfsk_core::msg::ApHint;
use mfsk_core::{Ft8, Mode};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

// mfsk-core 0.13's `Decoder<Ft8>` owns the callsign hash table and the
// plausibility filter, so the hand-rolled registration this file used to do is
// gone. There is no staged API: "subtract" is one `SicEarly` decode, which
// already contains what a first single pass would find. `osd: true` stays on
// unconditionally — it is the axis that actually buys recall (see ft8-web).

/// Decoded message returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedMessage {
    pub freq_hz: f32,
    pub dt_sec: f32,
    pub snr_db: f32,
    pub hard_errors: u32,
    pub pass: u8,
    pub message: String,
}

/// Shared decoder state: one persistent decoder (hash table for AP, a7).
pub struct DecoderState {
    decoder: Mutex<Decoder<Ft8>>,
}

impl DecoderState {
    pub fn new() -> Self {
        Self {
            decoder: Mutex::new(Decoder::with_defaults()),
        }
    }

    fn decode(
        &self,
        audio: &[i16],
        band: (f32, f32),
        sniper: Option<f32>,
        sync_min: f32,
        max_cand: usize,
        strategy: Ft8Strategy,
        strictness: DecodeStrictness,
        eq: EqMode,
        ap_hint: Option<ApHint>,
    ) -> Vec<DecodedMessage> {
        let mut d = self.decoder.lock().unwrap();
        let mut p = default_params(Mode::Ft8).band(band.0, band.1).depth(Depth::Deep);
        if let Some(t) = sniper {
            p = p.rx_freq(t);
        }
        *d.params_mut() = p;
        *d.extras_mut() = Ft8Extras {
            tuning: Tuning {
                sync_min: Some(sync_min),
                max_cand: Some(max_cand),
                osd: Some(true),
                strictness: Some(strictness),
                strategy: Some(strategy),
            },
            ap_hint,
            eq,
            sniper: sniper.map(|_| Sniper::default()),
            ..Ft8Extras::default()
        };
        let mut seen = std::collections::HashSet::new();
        d.decode(&SlotInput::i16(audio))
            .rows
            .iter()
            .filter(|r| seen.insert(r.decoded.text.clone()))
            .map(to_message)
            .collect()
    }
}

fn to_message<R>(r: &Row<R>) -> DecodedMessage {
    DecodedMessage {
        freq_hz: r.decoded.freq_hz,
        dt_sec: r.decoded.dt_sec,
        snr_db: r.decoded.snr_db,
        hard_errors: r.detail.hard_errors,
        pass: r.detail.pass,
        message: r.decoded.text.clone(),
    }
}

/// Normalize f32 samples to a target peak before converting to i16.
///
/// FT8 signals from hardware are often at very low absolute levels
/// (< 0.01 f32) depending on the audio adapter's gain.  A direct
/// `s * 32767` conversion would leave i16 values near ±300, wasting 6–7
/// bits and degrading SNR in the decoder's integer math.
///
/// This function scales the buffer so the peak reaches TARGET_PEAK (0.8),
/// preserving signal-to-noise ratio while making full use of the i16 range.
/// Pure noise is also scaled, so SNR is unchanged — only the absolute
/// amplitude is adjusted.  Buffers below the silence floor are left as-is.
fn normalize_to_i16(samples: &[f32]) -> Vec<i16> {
    const TARGET_PEAK: f32 = 0.8;
    const SILENCE_FLOOR: f32 = 1e-6;

    let peak = samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    let scale = if peak > SILENCE_FLOOR {
        TARGET_PEAK / peak
    } else {
        1.0
    };
    samples
        .iter()
        .map(|&s| (s * scale * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect()
}

fn to_strictness(level: u8) -> DecodeStrictness {
    match level {
        0 => DecodeStrictness::Strict,
        2 => DecodeStrictness::Deep,
        _ => DecodeStrictness::Normal,
    }
}

/// Wide-band decode (full 100-3000 Hz scan, single pass)
#[tauri::command]
pub fn decode_wideband(
    state: tauri::State<'_, DecoderState>,
    samples: Vec<f32>,
    strictness: u8,
) -> Vec<DecodedMessage> {
    let _ = strictness;
    let audio = normalize_to_i16(&samples);
    state.decode(
        &audio, (100.0, 3000.0), None, 1.5, 200,
        Ft8Strategy::SinglePass, DecodeStrictness::Normal, EqMode::Off, None,
    )
}

/// Wide-band decode with signal subtraction
#[tauri::command]
pub fn decode_subtract(
    state: tauri::State<'_, DecoderState>,
    samples: Vec<f32>,
    strictness: u8,
) -> Vec<DecodedMessage> {
    let audio = normalize_to_i16(&samples);
    state.decode(
        &audio, (100.0, 3000.0), None, 1.0, 200,
        Ft8Strategy::SicEarly, to_strictness(strictness), EqMode::Off, None,
    )
}

/// Sniper-mode decode with AP: narrow-band SIC + equalizer, matching
/// ft8-web's `sniper_decode`.
#[tauri::command]
pub fn decode_sniper(
    state: tauri::State<'_, DecoderState>,
    samples: Vec<f32>,
    target_freq: f32,
    callsign: String,
    mycall: String,
    eq_on: bool,
) -> Vec<DecodedMessage> {
    let audio = normalize_to_i16(&samples);

    let ap = if callsign.is_empty() {
        None
    } else if mycall.is_empty() {
        Some(ApHint::new().with_call1("CQ").with_call2(&callsign))
    } else {
        Some(ApHint::new().with_call1(&mycall).with_call2(&callsign))
    };

    let eq_mode = if eq_on { EqMode::Local } else { EqMode::Off };
    state.decode(
        &audio,
        ((target_freq - 250.0).max(100.0), (target_freq + 250.0).min(5900.0)),
        Some(target_freq), 0.8, 20,
        Ft8Strategy::SicEarly, DecodeStrictness::Normal, eq_mode, ap,
    )
}

/// Encode FT8 TX waveform
#[tauri::command]
pub fn encode_ft8(
    call1: String,
    call2: String,
    report: String,
    freq_hz: f32,
) -> Result<Vec<f32>, String> {
    use mfsk_core::msg::wsjt77::pack77;

    let msg77 = pack77(&call1, &call2, &report).ok_or("Failed to pack message")?;
    let tones = message_to_tones::<Ft8>(&msg77);
    Ok(synthesize::<Ft8>(&tones, 12_000, freq_hz, 1.0))
}
