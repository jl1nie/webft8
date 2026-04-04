/// Evaluate the ft8-core decoder against real recorded FT8 WAV files.
///
/// Reference recordings from jl1nie/RustFT8:
///   data/191111_110130.wav  (15 s, 12000 Hz, 16-bit PCM mono)
///   data/191111_110200.wav
use std::path::Path;

use ft8_core::decode::{decode_frame, DecodeDepth, DecodeResult};

// ────────────────────────────────────────────────────────────────────────────

pub struct RealDataReport {
    pub wav_path: String,
    pub sample_rate: u32,
    pub num_samples: usize,
    pub messages: Vec<DecodeResult>,
}

impl RealDataReport {
    pub fn print(&self) {
        println!("=== {} ===", self.wav_path);
        println!(
            "  WAV: {} Hz, {} samples ({:.1} s)",
            self.sample_rate,
            self.num_samples,
            self.num_samples as f64 / self.sample_rate as f64
        );
        println!("  Decoded: {} message(s)", self.messages.len());
        for (i, r) in self.messages.iter().enumerate() {
            // Pack 77 individual bits into 10 bytes (MSB first), print as hex.
            let mut packed = [0u8; 10];
            for (j, &bit) in r.message77.iter().enumerate() {
                packed[j / 8] |= (bit & 1) << (7 - j % 8);
            }
            let bits_hex: String = packed.iter().map(|b| format!("{b:02x}")).collect();
            println!(
                "  [{i:2}] freq={:7.1} Hz  dt={:+.2} s  errors={:2}  pass={}  msg={bits_hex}",
                r.freq_hz, r.dt_sec, r.hard_errors, r.pass
            );
        }
        println!();
    }
}

// ────────────────────────────────────────────────────────────────────────────

/// Decode a real 15-second WAV file over the full FT8 band (200–2800 Hz).
///
/// The WAV must be mono 16-bit PCM at 12 000 Hz (standard FT8 audio).
pub fn evaluate_real_data(wav_path: &Path) -> Result<RealDataReport, String> {
    let mut reader =
        hound::WavReader::open(wav_path).map_err(|e| format!("open WAV: {e}"))?;

    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(format!(
            "expected mono WAV, got {} channels",
            spec.channels
        ));
    }
    if spec.sample_rate != 12_000 {
        return Err(format!(
            "expected 12000 Hz WAV, got {} Hz",
            spec.sample_rate
        ));
    }

    let samples: Vec<i16> = match spec.sample_format {
        hound::SampleFormat::Int if spec.bits_per_sample == 16 => reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("read samples: {e}"))?,
        hound::SampleFormat::Int if spec.bits_per_sample == 8 => reader
            .samples::<i8>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("read samples: {e}"))?
            .into_iter()
            .map(|s| (s as i16) << 8)
            .collect(),
        _ => {
            return Err(format!(
                "unsupported WAV format: {:?} {}bps",
                spec.sample_format, spec.bits_per_sample
            ))
        }
    };

    let num_samples = samples.len();

    let messages = decode_frame(
        &samples,
        200.0,   // freq_min
        2800.0,  // freq_max
        1.5,     // sync_min — standard WSJT-X threshold
        None,    // no frequency hint
        DecodeDepth::BpAllOsd,
        200,     // max_cand — generous for full-band scan
    );

    Ok(RealDataReport {
        wav_path: wav_path.display().to_string(),
        sample_rate: spec.sample_rate,
        num_samples,
        messages,
    })
}
