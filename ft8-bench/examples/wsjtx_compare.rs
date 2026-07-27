/// Head-to-head: dump full decode_frame() text output for a WAV so it can
/// be diffed directly against `jt9 -8 -d3`'s output on the same file.
use std::path::PathBuf;

use mfsk_core::ft8::Ft8;
use mfsk_core::ft8::decode::{DecodeDepth, DecodeResult, DecodeStrictness};
use mfsk_core::ft8::message::unpack77;
use mfsk_core::msg::decode_request::DecodeRequest;

/// See ft8-web/src/lib.rs's SHIPPED_DEPTH doc comment: the 2026-07-26
/// depth-matrix investigation concluded FULL stays the default (Minimal
/// was worse on a dense scenario).
const SHIPPED_DEPTH: DecodeDepth = DecodeDepth::FULL;

fn main() {
    let wav_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: wsjtx_compare <wav> [subtract|phase2_shape]");
    let mode = std::env::args().nth(2).unwrap_or_default();
    let use_subtract = mode == "subtract";
    let use_phase2_shape = mode == "phase2_shape";

    let mut reader = hound::WavReader::open(&wav_path).expect("open WAV");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 12_000, "expected 12000 Hz WAV");
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .expect("read samples");

    let results: Vec<DecodeResult> = if use_phase2_shape {
        // `known(&[]).staged()` — exactly what ft8-web's decode_phase2 hits
        // when Phase 1 found nothing to seed. Prior to mfsk-core commit
        // fe286cc / issue #191, this call shape (the only one accepting
        // `known`/`fft_cache`) ran a separate, unfixed flat-3-pass engine
        // that structurally diverged from `decode_frame_subtract`'s staged
        // one. `known`/`fft_cache` are now honoured directly by `.staged()`,
        // so this should match plain `subtract` mode message-for-message.
        let known: Vec<DecodeResult> = Vec::new();
        DecodeRequest::<Ft8>::new(&samples, 200.0, 3000.0, 1.5, 200)
            .depth(SHIPPED_DEPTH)
            .strictness(DecodeStrictness::Normal)
            .known(&known)
            .staged()
            .decode()
            .results
    } else if use_subtract {
        DecodeRequest::<Ft8>::new(&samples, 200.0, 3000.0, 1.5, 200)
            .depth(SHIPPED_DEPTH)
            .strictness(DecodeStrictness::Normal)
            .staged()
            .decode()
            .results
    } else {
        DecodeRequest::<Ft8>::new(&samples, 200.0, 3000.0, 1.5, 200)
            .depth(SHIPPED_DEPTH)
            .decode()
            .results
    };
    let label = if use_phase2_shape { "decode_phase2 shape (known+staged)" } else if use_subtract { "decode_frame_subtract (staged)" } else { "decode_frame" };
    println!("mfsk-core {}: {} message(s)", label, results.len());
    let mut sorted = results;
    sorted.sort_by(|a, b| a.freq_hz.partial_cmp(&b.freq_hz).unwrap());
    for r in &sorted {
        let text = unpack77(r.message77()).unwrap_or_default();
        println!(
            "{:+4.0} {:+4.1} {:4.0} ~  {}",
            r.snr_db, r.dt_sec, r.freq_hz, text
        );
    }
}
