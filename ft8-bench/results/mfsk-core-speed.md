# mfsk-core 速度比較: 0.6系 → GitHub main (0.7.4)

## 背景

2026-07-25、`mfsk-core` の依存を crates.io 版 (0.6.7) から GitHub
[jl1nie/mfsk-core](https://github.com/jl1nie/mfsk-core) の `main` ブランチ
(未公開の高速化コミットを含む) に切り替えた。実際にどれだけ速くなったかを
`ft8-bench/examples/speed_bench.rs` で計測した。

## Setup

- WAV: `qso3_busy.wav`(実録音、混雑バンド、12000 Hz mono、15 s)
  — リポジトリには含めない実録音 fixture のため各自コピーして実行
- デコード対象: `decode_frame(200–2800 Hz, dt_tol=1.5s, DecodeDepth::BpAllOsd, max_iter=200)`
- 試行回数: 20 回(1 回のウォームアップの後)、中央値で比較
- 計測コマンド: `cargo run --release -p ft8-bench --example speed_bench`

## 結果 (2026-07-25)

| mfsk-core | バージョン | デコード時間 (中央値) | デコード件数 |
|---|---|---|---|
| crates.io | 0.6.8 | 98.7 ms | 11 msgs |
| GitHub main | 0.7.4 (`28a1f03f`) | 14.3 ms | 13 msgs |

**約 6.9 倍の高速化**。速度だけでなくデコード件数も増加(11 → 13)しており、
検出精度も同時に改善している。

## 反映状況

- WebFT8 (`ft8-web`) / uvpacket-web の WASM ビルドは 2026-07-25 に
  GitHub main (`28a1f03f`) でリビルド・`docs/` へデプロイ済み(GitHub Pages に反映)。
- ft8-desktop (Tauri) 側も同じ GitHub 参照に切り替え済み(v0.5.5)。ローカル環境の
  制約でこの環境では `.exe`/`.msi` のビルド検証はできていない。

## Files

| File | Description |
|------|--------------|
| `../examples/speed_bench.rs` | この計測に使ったベンチマークスクリプト |
