# WebFT8 — FT8 in Your Browser

**[日本語版](README.jp.md)** | **[Open App](https://jl1nie.github.io/webft8/)** | **[Manual](docs/manual.en.md)** | **[Library reference](docs/LIBRARY.md)**

> Pure Rust FT8 decoder running as a WASM PWA.
> No install, no Java — just open and operate.

## Features

- **Multi-mode** — decodes FT8 / FT4 / FST4 / WSPR / Q65. **FT8 / FT4 / Q65 support full QSO** (auto-sequence: IDLE → CALLING → REPORT → FINAL) — WSPR and FST4 run as receive-only monitors
- **Sniper mode** — 500 Hz hardware BPF + adaptive equalizer for extreme weak-signal DX (FT8)
- **Pipelined decode** — Phase 1 results shown instantly, Phase 2 adds subtract signals
- **CAT control** — Yaesu / Icom PTT via Web Serial API or Bluetooth LE
- **Works everywhere** — PC, tablet, smartphone. Chrome, Edge, Safari
- **Offline-capable PWA** — install to home screen, works without network
- **WAV analysis / live recording** — drag & drop any WAV for offline decode, or record received audio live per-slot (Chrome/Edge)

## Quick Start

1. **[Open WebFT8](https://jl1nie.github.io/webft8/)**
2. Allow microphone access
3. Enter your callsign and grid in Settings (gear icon)
4. Select audio input/output → **Start Audio**
5. Connect radio via USB or BLE for CAT control (optional)

**Offline trial:** drag & drop a [test WAV](https://github.com/jl1nie/webft8/raw/main/ft8-bench/testdata/sim_busy_band.wav) onto the waterfall.

## Two Modes

| Mode | Purpose | Use case |
|------|---------|----------|
| **Scout** | Chat-style UI, tap to call | Casual CQ, portable, mobile |
| **Snipe** | DX hunting, target lock | DXpedition pileup, weak signal |

## Sniper Mode — The Differentiator

Standard FT8 apps (WSJT-X, JTDX) decode across a 3 kHz band. When a +40 dB station is present, the 16-bit ADC buries weak signals in quantization noise.

WebFT8's sniper mode uses the transceiver's **500 Hz hardware narrow filter** to physically remove strong interference *before* the ADC, then applies:

1. **Adaptive equalizer** — corrects BPF edge distortion using Costas pilot tones
2. **Successive interference cancellation** — 3-pass subtract with QSB gate
3. **A Priori decoding** — locks known callsign bits (up to 77-bit full lock)

## vs WSJT-X

| Feature | WSJT-X | WebFT8 |
|---------|--------|--------|
| Platform | Desktop (Java/Fortran) | **Browser (Rust/WASM)** |
| BPF integration | None | **500 Hz sniper mode** |
| Equalizer | None | **Costas Wiener adaptive EQ** |
| Parallelism | Serial | **Rayon par_iter (7.7x)** |
| Subtract | 4-pass | **3-pass + QSB gate** |
| Binary size | ~120 MB | **1.51 MB / 432 KB gzip (full PWA)** |

### Decode Comparison (15 crowd stations + weak target)

| Scenario | WSJT-X | WebFT8 |
|----------|--------|--------|
| crowd +5 dB, target −12 dB | 7 decoded | **16 decoded** |
| crowd +40 dB, target −14 dB (54 dB gap) | 0% | **100% with 500 Hz HW BPF** |
| BPF edge −18 dB, no AP | — | **100% (EQ)** |
| BPF edge −20 dB, EQ + AP | — | **100%** |
| Butterworth vs Elliptic 4-pole (center, −20 dB) | — | **100% both** |

Numbers re-verified 2026-08-02 against mfsk-core 0.8.1 (unchanged); see [docs/bench.md](docs/bench.md) for details and the decode-engine history.

Full benchmark data (all scenarios, SNR sweeps, filter comparison): **[docs/bench.md](docs/bench.md)**

## For Developers

```
webft8/
├── ft8-bench/     Benchmark & simulation suite
├── ft8-web/       WASM bindings + PWA frontend (decode engine: mfsk-core, github.com/jl1nie/mfsk-core)
├── ft8-desktop/   Tauri native wrapper
└── docs/          GitHub Pages deployment
```

### Build

```bash
# Native
cargo build --release
cargo run -p ft8-bench --release    # benchmarks + simulation

# WASM
cd ft8-web && wasm-pack build --target web --release
```

19 unit tests (ft8-bench 9 + uvpacket-web 9 + 1 integration test; `cargo test --workspace`. ft8-web/ft8-desktop are thin binding layers with no tests of their own — the decode engine, mfsk-core, is tested separately in its own upstream repo). WASM binary 1.23 MB (339 KB gzip).

## References

- [WSJT-X](https://github.com/saitohirga/WSJT-X) — FT8 reference implementation
- K1JT et al., "The FT4 and FT8 Communication Protocols", QEX, 2020

## License

GPL-3.0-or-later — includes ported algorithms from WSJT-X.
