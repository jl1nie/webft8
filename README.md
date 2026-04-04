# rs-ft8n — FT8 Sniper-Mode Decoder in Rust

A next-generation FT8 decoder that couples a **500 Hz hardware narrowband filter** with a software decoder to achieve decodes that WSJT-X cannot — even in environments with a strong adjacent QRM (+40 dB).

## コンセプト / Concept

通常の広帯域 ADC（16bit / 3kHz）では、+40dB 以上の隣接 QRM が存在すると、ターゲット信号が ADC の量子化ノイズに埋没する。本プロジェクトは量子化の**前段**に 500Hz 物理フィルタを置き、ADC の全ダイナミックレンジをターゲット信号に集中させる「スナイパー・モード」を実現する。

In a wideband (3 kHz) ADC, a +40 dB adjacent signal consumes nearly all 16-bit dynamic range, burying the target in quantization noise. By placing a **500 Hz hardware BPF before the ADC**, the full dynamic range is devoted to the target — this is the "Sniper Mode" concept.

```
[Antenna] → [500Hz BPF] → [ADC 16bit] → rs-ft8n → decoded FT8 message
             ↑ removes +40dB QRM before digitisation
```

## デコード性能 / Decode Performance

Verified against real recordings (`191111_110200.wav` from [jl1nie/RustFT8](https://github.com/jl1nie/RustFT8)):

| Signal | SNR | WSJT-X | rs-ft8n | Method |
|--------|-----|--------|---------|--------|
| CQ R7IW LN35 | −8 dB | ✓ | ✓ | BP |
| CQ TA6CQ KN70 | −8 dB | ✓ | ✓ | BP |
| OH3NIV ZS6S RR73 | **−14 dB** | ✓ | ✓ | **OSD ord-3** |
| CQ LZ1JZ KN22 | **−15 dB** | ✓ | ✓ | **OSD ord-2** |
| (extra signal @ 2096.9 Hz) | — | ✗ | ✓ | BP |

## アーキテクチャ / Architecture

```
rs-ft8n/
├── ft8-core/          Pure Rust FT8 decode library (no_std ready)
│   └── src/
│       ├── params.rs       FT8 protocol constants
│       ├── downsample.rs   FFT-based 12kHz→200Hz complex baseband
│       ├── sync.rs         2-D Costas correlation + parabolic fine sync
│       ├── llr.rs          Soft-decision LLR (4 metric variants a/b/c/d)
│       ├── decode.rs       End-to-end pipeline; BP + OSD depth control
│       └── ldpc/
│           ├── bp.rs       Log-domain Belief Propagation (30 iter)
│           ├── osd.rs      Ordered Statistics Decoding (order 1-3)
│           └── tables.rs   LDPC(174,91) parity-check matrix
└── ft8-bench/         Benchmark & evaluation harness
    └── src/
        ├── real_data.rs    Full-band WAV evaluation
        └── diag.rs         Per-signal pipeline trace
```

## デコードパイプライン / Decode Pipeline

```
PCM 16bit 12kHz
  │
  ▼ downsample (FFT, Hann window)
Complex baseband 200 Hz
  │
  ▼ coarse_sync (Costas correlation, 2-D grid)
Candidate list (freq, dt, score)
  │
  ▼ refine_candidate (sub-symbol fine sync + parabolic interpolation)
  │
  ▼ symbol_spectra (32-pt FFT × 79 symbols)
  │
  ▼ sync_quality (hard-decision Costas check, 0-21)
  │
  ▼ compute_llr (Gray-coded soft metrics, 4 variants)
  │
  ├─▶ BP decode (log-domain tanh, 30 iter, CRC-14)
  │     success → DecodeResult (pass=0..3)
  │
  └─▶ OSD fallback (when BP fails, sync_q≥12, score≥2.5)
        order-2 (~4,187 candidates) for sync_q < 18
        order-3 (~121,667 candidates) for sync_q ≥ 18
        success → DecodeResult (pass=4)
```

## ビルド / Build

```bash
cargo build --release
```

依存クレート: `rustfft`, `num-complex`, `crc`, `hound`

## 使い方 / Usage

### ベンチマーク実行 / Run Benchmark

```bash
# テストデータを配置 / Place test WAVs:
# ft8-bench/testdata/191111_110130.wav
# ft8-bench/testdata/191111_110200.wav
# (from https://github.com/jl1nie/RustFT8/tree/main/data)

cargo run -p ft8-bench --release
```

### ライブラリとして使う / Use as Library

```rust
use ft8_core::decode::{decode_frame, DecodeDepth};

let samples: Vec<i16> = /* 12000 Hz PCM */;
let messages = decode_frame(
    &samples,
    200.0,              // freq_min (Hz)
    2800.0,             // freq_max (Hz)
    1.5,                // sync_min threshold
    None,               // freq_hint
    DecodeDepth::BpAllOsd,  // BP + OSD fallback
    200,                // max candidates
);

for msg in &messages {
    println!("{:.1} Hz  dt={:+.2}s  errors={}  pass={}", 
             msg.freq_hz, msg.dt_sec, msg.hard_errors, msg.pass);
}
```

スナイパーモード（500Hz フィルタ後の信号に）:

```rust
use ft8_core::decode::{decode_sniper, DecodeDepth};

let messages = decode_sniper(&samples, 1850.0, DecodeDepth::BpAllOsd, 50);
```

## 技術詳細 / Technical Notes

### Belief Propagation (BP)
WSJT-X `bpdecode174_91.f90` から移植。log-domain tanh メッセージパッシング、最大 30 反復、早期停止付き。

### Ordered Statistics Decoding (OSD)
WSJT-X `osd174_91.f90` から移植。

1. |LLR| 降順でビットを整列
2. 系統的生成行列を置換・GF(2) ガウス消去 → 最信頼基底 (MRB) を特定
3. MRB の硬判定符号語（order-0）+ 1〜3 ビット反転候補を列挙
4. CRC-14 を通過した最小重み符号語を返す

SNR −15 dB の信号を order-2 で、−14 dB を order-3 で回収。

### LDPC(174, 91)
パリティ検査行列は WSJT-X `ldpc_174_91_c_parity.f90` から移植。生成行列は `ldpc_174_91_c_generator.f90` から移植。

## ロードマップ / Roadmap

- [x] Phase 1: 基本デコードパイプライン (BP)
- [x] Phase 2: 実データ評価 + OSD フォールバック → WSJT-X 同等
- [ ] Phase 3: 適応型等化器 (500Hz フィルタエッジ補正)
- [ ] Phase 4: Signal subtract (2nd パス再デコード)
- [ ] Phase 5: WASM 化 (Web Audio API 対応)

## 参考 / References

- [WSJT-X](https://github.com/saitohirga/WSJT-X) — FT8 Fortran reference implementation
- [jl1nie/RustFT8](https://github.com/jl1nie/RustFT8) — test WAV data source
- K1JT et al., "FT8, a Weak-Signal Mode for HF DXing", QST, 2018

## ライセンス / License

MIT
