use ft8_core::decode::{
    decode_frame, decode_frame_subtract, decode_sniper_ap,
    ApHint, DecodeDepth, DecodeStrictness, EqMode,
};
use ft8_core::hash_table::CallsignHashTable;
use ft8_core::message::{is_plausible_message, unpack77_with_hash};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

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

/// Shared decoder state (hash table for AP)
pub struct DecoderState {
    hash_table: Mutex<CallsignHashTable>,
}

impl DecoderState {
    pub fn new() -> Self {
        Self {
            hash_table: Mutex::new(CallsignHashTable::new()),
        }
    }

    fn decode_and_register(
        &self,
        results: Vec<ft8_core::decode::DecodeResult>,
    ) -> Vec<DecodedMessage> {
        let mut ht = self.hash_table.lock().unwrap();
        let mut out = Vec::new();
        for r in results {
            if let Some(text) = unpack77_with_hash(&r.message77, &ht) {
                if text.is_empty() || !is_plausible_message(&text) {
                    continue;
                }
                // Register callsigns for AP
                for word in text.split_whitespace() {
                    if matches!(
                        word,
                        "CQ" | "DE" | "QRZ" | "DX" | "RRR" | "RR73" | "73" | "R" | ""
                    ) {
                        continue;
                    }
                    if word.starts_with("CQ")
                        || word.starts_with('<')
                        || word.starts_with('+')
                        || word.starts_with('-')
                        || word.starts_with("R+")
                        || word.starts_with("R-")
                        || word.starts_with('[')
                    {
                        continue;
                    }
                    if word.len() == 4 {
                        let b = word.as_bytes();
                        if b[0].is_ascii_uppercase()
                            && b[1].is_ascii_uppercase()
                            && b[2].is_ascii_digit()
                            && b[3].is_ascii_digit()
                        {
                            continue;
                        }
                    }
                    ht.insert(word);
                }
                out.push(DecodedMessage {
                    freq_hz: r.freq_hz,
                    dt_sec: r.dt_sec,
                    snr_db: r.snr_db,
                    hard_errors: r.hard_errors,
                    pass: r.pass,
                    message: text,
                });
            }
        }
        out
    }
}

fn to_strictness(level: u8) -> DecodeStrictness {
    match level {
        0 => DecodeStrictness::Strict,
        2 => DecodeStrictness::Deep,
        _ => DecodeStrictness::Normal,
    }
}

/// Wide-band decode (full 100-3000 Hz scan)
#[tauri::command]
pub fn decode_wideband(
    state: tauri::State<'_, DecoderState>,
    samples: Vec<f32>,
    strictness: u8,
) -> Vec<DecodedMessage> {
    let _ = strictness;
    // ft8-core expects i16 for decode_frame
    let audio: Vec<i16> = samples
        .iter()
        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect();
    let results = decode_frame(&audio, 100.0, 3000.0, 1.5, None, DecodeDepth::BpAllOsd, 200);
    state.decode_and_register(results)
}

/// Wide-band decode with signal subtraction
#[tauri::command]
pub fn decode_subtract(
    state: tauri::State<'_, DecoderState>,
    samples: Vec<f32>,
    strictness: u8,
) -> Vec<DecodedMessage> {
    let audio: Vec<i16> = samples
        .iter()
        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect();
    let results = decode_frame_subtract(
        &audio,
        100.0,
        3000.0,
        1.0,
        None,
        DecodeDepth::BpAllOsd,
        200,
        to_strictness(strictness),
    );
    state.decode_and_register(results)
}

/// Sniper-mode decode with AP
#[tauri::command]
pub fn decode_sniper(
    state: tauri::State<'_, DecoderState>,
    samples: Vec<f32>,
    target_freq: f32,
    callsign: String,
    mycall: String,
    eq_on: bool,
) -> Vec<DecodedMessage> {
    let audio: Vec<i16> = samples
        .iter()
        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect();

    let eq_mode = if eq_on { EqMode::Adaptive } else { EqMode::Off };

    let ap = if callsign.is_empty() {
        None
    } else if mycall.is_empty() {
        Some(ApHint::new().with_call1("CQ").with_call2(&callsign))
    } else {
        Some(ApHint::new().with_call1(&mycall).with_call2(&callsign))
    };

    let results =
        decode_sniper_ap(&audio, target_freq, DecodeDepth::BpAllOsd, 20, eq_mode, ap.as_ref());
    state.decode_and_register(results)
}

/// Encode FT8 TX waveform
#[tauri::command]
pub fn encode_ft8(
    call1: String,
    call2: String,
    report: String,
    freq_hz: f32,
) -> Result<Vec<f32>, String> {
    use ft8_core::message::pack77;
    use ft8_core::wave_gen::{message_to_tones, tones_to_f32};

    let msg77 = pack77(&call1, &call2, &report).ok_or("Failed to pack message")?;
    let tones = message_to_tones(&msg77);
    Ok(tones_to_f32(&tones, freq_hz, 1.0))
}
