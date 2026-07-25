/// Head-to-head: dump full decode_frame() text output for a WAV so it can
/// be diffed directly against `jt9 -8 -d3`'s output on the same file.
use std::path::PathBuf;

use mfsk_core::ft8::decode::{decode_frame, decode_frame_subtract, DecodeDepth, DecodeStrictness};
use mfsk_core::ft8::message::unpack77;

fn main() {
    let wav_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: wsjtx_compare <wav> [subtract]");
    let use_subtract = std::env::args().nth(2).as_deref() == Some("subtract");

    let mut reader = hound::WavReader::open(&wav_path).expect("open WAV");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 12_000, "expected 12000 Hz WAV");
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .expect("read samples");

    let results = if use_subtract {
        decode_frame_subtract(&samples, 200.0, 3000.0, 1.5, None, DecodeDepth::BpAllOsd, 200, DecodeStrictness::Normal)
    } else {
        decode_frame(&samples, 200.0, 3000.0, 1.5, None, DecodeDepth::BpAllOsd, 200)
    };
    println!("mfsk-core {}: {} message(s)", if use_subtract { "decode_frame_subtract" } else { "decode_frame" }, results.len());
    let mut sorted = results;
    sorted.sort_by(|a, b| a.freq_hz.partial_cmp(&b.freq_hz).unwrap());
    for r in &sorted {
        let text = unpack77(&r.message77).unwrap_or_default();
        println!(
            "{:+4.0} {:+4.1} {:4.0} ~  {}",
            r.snr_db, r.dt_sec, r.freq_hz, text
        );
    }
}
