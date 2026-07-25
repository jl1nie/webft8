/// Decode-speed benchmark: full-band decode_frame() timing on a fixed WAV.
use std::path::PathBuf;
use std::time::Instant;

use mfsk_core::ft8::decode::{decode_frame, DecodeDepth};

fn main() {
    let wav_path = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/qso3_busy.wav")
    });
    let iters: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    if !wav_path.exists() {
        println!(
            "SKIP: {} not found (real-recording WAVs aren't checked in — copy one in, \
             e.g. mfsk-core's embedded-poc/assets/qso3_busy.wav, or pass a path as arg 1)",
            wav_path.display()
        );
        return;
    }

    let mut reader = hound::WavReader::open(&wav_path).expect("open WAV");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 12_000, "expected 12000 Hz WAV");
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .expect("read samples");

    // Warm-up (page-in, allocator warm, branch predictor etc.)
    let n = decode_frame(&samples, 200.0, 2800.0, 1.5, None, DecodeDepth::BpAllOsd, 200).len();
    println!("warm-up decoded {n} message(s)");

    let mut durations = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        let msgs = decode_frame(&samples, 200.0, 2800.0, 1.5, None, DecodeDepth::BpAllOsd, 200);
        durations.push(t0.elapsed());
        std::hint::black_box(&msgs);
    }

    durations.sort();
    let total: std::time::Duration = durations.iter().sum();
    let mean = total / iters as u32;
    let median = durations[iters / 2];
    let min = durations[0];
    let max = durations[iters - 1];

    println!("wav={} iters={iters}", wav_path.display());
    println!(
        "mean={:.1}ms median={:.1}ms min={:.1}ms max={:.1}ms",
        mean.as_secs_f64() * 1000.0,
        median.as_secs_f64() * 1000.0,
        min.as_secs_f64() * 1000.0,
        max.as_secs_f64() * 1000.0,
    );
}
