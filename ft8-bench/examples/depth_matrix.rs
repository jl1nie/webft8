/// Sweeps LlrEffort x osd (now independent fields on DecodeDepth, since
/// mfsk-core's issue #188 redesign) against the real qso3_busy.wav
/// recording, for both the plain single-pass shape (decode_phase1-style)
/// and the staged-SIC shape (decode_phase2/decode_wav_subtract-style).
///
/// Ground truth: jt9 -8 -p 15 -d 3 qso3_busy.wav, 200-3000 Hz in-band,
/// verified directly this session (2026-07-26) — 21 confirmed messages,
/// excluding one out-of-band decode at 3390 Hz. See docs/bench.md's
/// WSJT-X Comparison section for how this list was derived.
use std::path::PathBuf;
use std::time::Instant;

use mfsk_core::ft8::Ft8;
use mfsk_core::ft8::decode::{DecodeDepth, DecodeResult, DecodeStrictness, LlrEffort};
use mfsk_core::ft8::message::unpack77;
use mfsk_core::msg::decode_request::DecodeRequest;

const JT9_CONFIRMED: &[&str] = &[
    "K1BZM DK8NE -10",
    "W0RSJ EA3BMU RR73",
    "N1PJT HB9CQK -10",
    "KD2UGC F6GCP R-23",
    "K1JT HA0DU KN07",
    "N1JFU EA6EE R-7",
    "A92EE F5PSR -14",
    "CQ F5RXL IN94",
    "N1API F2VX 73",
    "K1JT EA3AGB -15",
    "K1JT HA5WA 73",
    "WM3PEN EA6VQ -9",
    "N1API HA6FQ -23",
    "CQ EA2BFM IN83",
    "K1BZM EA3CJ JN01",
    "WA2FZW DL5AXX RR73",
    "W1FC F5BZB -8",
    "CQ DX DL8YHR JO41",
    "K1BZM EA3GP -9",
    "W1DIG SV9CVY -14",
    "XE2X HA2NP RR73",
];

fn matched_count(results: &[DecodeResult]) -> usize {
    results
        .iter()
        .filter_map(|r| unpack77(r.message77()))
        .filter(|text| JT9_CONFIRMED.iter().any(|m| text.starts_with(m)))
        .count()
}

fn main() {
    let wav_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/qso3_busy.wav")
        });
    if !wav_path.exists() {
        println!("SKIP: {} not found", wav_path.display());
        return;
    }
    let mut reader = hound::WavReader::open(&wav_path).expect("open WAV");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 12_000, "expected 12000 Hz WAV");
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .expect("read samples");

    let depths = [
        ("Minimal,noOSD", DecodeDepth { llr_effort: LlrEffort::Minimal, osd: false }),
        ("Minimal,OSD  ", DecodeDepth { llr_effort: LlrEffort::Minimal, osd: true }),
        ("Full,noOSD   ", DecodeDepth { llr_effort: LlrEffort::Full, osd: false }),
        ("Full,OSD     ", DecodeDepth { llr_effort: LlrEffort::Full, osd: true }),
    ];

    println!("=== {} — plain single-pass (decode_phase1-style) ===", wav_path.display());
    println!("jt9 -d3 ground truth: {} confirmed in-band messages\n", JT9_CONFIRMED.len());
    println!("{:16} {:>10} {:>10}", "depth", "matched", "time");
    for (name, depth) in depths {
        let t0 = Instant::now();
        let r = DecodeRequest::<Ft8>::new(&samples, 200.0, 3000.0, 1.5, 200)
            .depth(depth)
            .decode()
            .results;
        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{:16} {:>3}/{:<3}   {:>7.1} ms",
            name, matched_count(&r), JT9_CONFIRMED.len(), elapsed
        );
    }

    println!("\n=== {} — staged SIC (decode_phase2/decode_wav_subtract-style) ===", wav_path.display());
    println!("{:16} {:>10} {:>10}", "depth", "matched", "time");
    for (name, depth) in depths {
        let t0 = Instant::now();
        let r = DecodeRequest::<Ft8>::new(&samples, 200.0, 3000.0, 1.5, 200)
            .depth(depth)
            .strictness(DecodeStrictness::Normal)
            .staged()
            .decode()
            .results;
        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{:16} {:>3}/{:<3}   {:>7.1} ms",
            name, matched_count(&r), JT9_CONFIRMED.len(), elapsed
        );
    }
}
