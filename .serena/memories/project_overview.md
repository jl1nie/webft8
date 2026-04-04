# rs-ft8n Project Overview

## Purpose
Next-generation FT8 decoder in Rust implementing "sniper mode" — tightly coupling a 500Hz physical narrow band-pass filter with a software decoder. Targets amateur radio scenarios where strong adjacent signals (+40dB) cause WSJT-X to fail due to ADC dynamic range exhaustion.

## Key concept
Physical 500Hz BPF applied before ADC quantization removes strong interferers, recovering ADC dynamic range for the target signal. Software side provides adaptive equalization (Costas array as pilot), optimized sync, and signal subtraction.

## Tech stack
- Language: Rust (workspace with two crates)
- FFT: `rustfft = "6"`
- Complex math: `num-complex = "0.4"`
- CRC: `crc = "3"`
- WAV I/O (bench only): `hound = "3"`
- Target: native + WASM (wasm32-unknown-unknown via rustfft SIMD)

## Crate structure
```
rs-ft8n/
├── Cargo.toml           — workspace root
├── ft8-core/            — pure Rust FT8 decoder library
│   └── src/
│       ├── params.rs    — FT8/LDPC constants
│       ├── downsample.rs — FFT downsample 12kHz→200Hz complex baseband
│       ├── sync.rs      — coarse + fine sync (Costas correlation)
│       ├── llr.rs       — soft LLR from symbol spectra
│       ├── decode.rs    — pipeline: decode_frame, decode_sniper
│       ├── ldpc/
│       │   ├── tables.rs — parity-check matrix (MN, NM, NRW)
│       │   ├── bp.rs    — Belief Propagation decoder + CRC-14
│       │   └── osd.rs   — OSD fallback (stub)
│       ├── wave_gen.rs  — GFSK wave synthesis (stub)
│       └── equalizer.rs — adaptive equalizer (stub)
└── ft8-bench/           — test bench / evaluator
    └── src/
        └── main.rs      — (stub, Phase 2 target)
```

## FT8 protocol parameters
- 79 symbols (58 data + 21 sync), 160ms/symbol, 6.25 baud
- LDPC(174,91): 77-bit msg + 14-bit CRC = 91 info bits + 83 parity
- Costas array [3,1,4,0,6,5,2] at symbol positions 0, 36, 72
- GRAYMAP [0,1,3,2,5,6,4,7]
- Downsampled rate: 200Hz (3200 samples/frame, 32 samples/symbol)

## Phase status
- Phase 1 COMPLETE: ft8-core decoder pipeline, 18 unit tests passing
- Phase 2 TODO: ft8-bench — real WAV evaluation, 500Hz filter sim, signal subtract
- Phase 3 TODO: equalizer, OSD, double sync + parabolic interpolation

## Reference
- WSJT-X source: `/home/minoru/src/WSJT-X/lib/ft8/`
- Real test WAVs: `jl1nie/RustFT8` repo `data/191111_110130.wav`, `191111_110200.wav`
