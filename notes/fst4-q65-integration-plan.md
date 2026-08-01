# WebFT8 への FST4 / Q65 組み込みプラン

> 作成日: 2026-08-01 / 対象ブランチ: `claude/webft8-fst4-q65-integration-psb5up`
> 対象: `webft8` (ft8-web WASM + www フロント) / 依存: `mfsk-core` v0.8.0 (git rev `8f375a2`)

## 🔑 現状の重要な事実

WebFT8 は既に FT8 専用ではない。WASM 層とフロントは FT8 / FT4 / WSPR / Q65 を
end-to-end で搭載済み。

| モード | mfsk-core | WASM binding | フロント UI | 状態 |
|--------|-----------|--------------|-------------|------|
| FT8  | ✅ | ✅ | ✅ | 完成 |
| FT4  | ✅ | ✅ | ✅ | 完成 |
| WSPR | ✅ | ✅ | ✅ | 完成 |
| **Q65**  | ✅ | ✅ | ✅ | **既に統合済み（要検証）** |
| **FST4** | ✅ | ❌ | ❌ | **未着手（本タスクの主対象）** |

- **Q65 は既に統合済み** — `decode_q65_wav` 系 WASM（`ft8-web/src/lib.rs:733-860`）＋
  `index.html` のサブモード/fading/b90 UI（`:544-565`）＋ `getSlotMs()` の 30/60s タイミング。
  → 新規開発ではなく **動作検証** の対象。
- **本当の作業は FST4** — `mfsk-core` は FST4 5サブモードを実装済みだが、`ft8-web` 側で
  feature 有効化もバインディングもされていない（`grep -ri fst4` → ヒットなし）。
- **FST4W は mfsk-core 未実装**（別フォーマットの 50bit LDPC(240,74)、mfsk-core issue #23）。
  → 今回のスコープ外。

## mfsk-core 側の FST4 API（FT4 と同一の汎用経路）

- 5サブモード ZST: `Fst4s15 / Fst4s30 / Fst4s60 / Fst4s120 / Fst4s300`
  （T/R 周期 15 / 30 / 60 / 120 / 300 秒）。全て `ProtocolId::Fst4`。
- 4-GFSK、LDPC(240,101)+CRC-24、77bit WSJT メッセージ（FT8/FT4 と同一 unpack 経路）。
- デコード: `mfsk_core::msg::decode_request::DecodeRequest::<Fst4s60>::new(`
  `audio: &[i16], freq_min: f32, freq_max: f32, sync_min: f32, max_cand: usize).decode()`。
  **i16 音声・12kHz**。結果は共有 `engine::pipeline::DecodeResult`、テキストは
  `msg::wsjt77::unpack77(r.message77())`。→ **既存 FT4 バインディングのほぼ複製**。
- エンコード: `mfsk_core::fst4::encode::{message_to_tones, tones_to_f32}`。
- **FST4 SIC(subtract) は未実装**（issue #193）→ subtract 系バインディングは作らない。
- **wide-band AP も FST4 未実装** → AP は sniper 経路のみ。

## 作業内容

### A. mfsk-core feature 有効化
`ft8-web/Cargo.toml`: `features = ["ft8","ft4","wspr","q65"]` に **`"fst4"` を追加**（rev 据え置き）。

### B. WASM バインディング — `ft8-web/src/lib.rs`
Q65 の `dispatch_q65_submode!` と同型で:
```rust
// submode 0..=4 → Fst4s15 / Fst4s30 / Fst4s60 / Fst4s120 / Fst4s300
pub fn decode_fst4_wav(samples: &[i16], submode: u8, sample_rate: u32) -> Vec<DecodedMessage>
pub fn decode_fst4_wav_f32(samples: &[f32], submode: u8, sample_rate: u32) -> Vec<DecodedMessage>
pub fn decode_fst4_sniper(...)   // DecodeRequest::sniper — 任意
pub fn encode_fst4(call1, call2, report, freq_hz, submode) -> Result<Vec<f32>, JsValue>
```
- `DecodeRequest::<P>::new(&audio, freq_min, freq_max, sync_min, max_cand)` を呼び、
  結果は既存 `ft4_decode_and_register()` / `DecodedMessage` を再利用。
- 周波数レンジ・`sync_min`・`max_cand` は FT4 値ベースで調整。

### C. Worker 登録 — `ft8-web/www/decode-worker.js`
`import { ... }` ブロックと `FN_MAP` の両方に FST4 関数を追加（Q65 と同じ2箇所）。

### D. フロントエンド — `app.js` + `index.html`
- `index.html`: モード選択に `<option value="fst4">FST4 (15/30/60/120/300 s)</option>` と、
  Q65 と同型の FST4 サブモード `<select>`（5サブモード）を追加。
- `app.js`:
  - `currentProtocol()` の許可リスト（`app.js:62`）に `'fst4'` を追加
  - `currentFst4Submode()` ヘルパ + localStorage
  - `getSlotMs()`（`app.js:83`）に FST4 分岐: 15000/30000/60000/120000/300000
  - `runDecode()`（`app.js:932` 付近）に `fst4` ブランチ → `decode_fst4_wav(_f32)` 呼び出し
  - モード/サブモード変更ハンドラで `periodMgr.setSlotMs(getSlotMs())` を反映（既存機構）

### E. ⚠️ ライブ音声バッファの可変長化（FST4-30 以上で必須・既存 Q65-60/WSPR も同時修正）
唯一の非自明なブロッカー:
- `audio-processor.js:28` のスナップショットバッファが **15秒固定**（`outputRate * 15`）。
- `FT8PeriodManager` は任意 `slotMs` 対応済みだが、**バッファ容量が足りず 15秒より長いスロットは
  末尾15秒に切り詰められる**。現状 WSPR(120s)/Q65-60 のライブも実質機能していない
  （"WAV-drop path only" 注記）。FST4-30/60/120/300 も同じ制約。
- 対応: `processorOptions`（またはメッセージ）でスロット秒数を渡し、`bufferSize` を
  選択周期に合わせて確保・再構築。メモリは 300s×12k×f32 ≈ **14.4MB**。
- `lib.rs` の Q65/FST4 検索パラメータは WAV-drop 調整値なのでライブ用値も検討。
- **FST4-15 は現行15秒バッファでそのまま動作**（Phase 1 では触らない）。

### F. ビルド & デプロイ
- `cd ft8-web && wasm-pack build --target web --release`（または `deploy.sh`）→
  `docs/ft8_web.js` / `ft8_web_bg.wasm` をペアで更新（CLAUDE.md §7.3）。
- バージョン: `ft8-desktop/src-tauri/Cargo.toml` の `version` を **0.6.0 → 0.7.0**
  （機能追加・後方互換ありの minor bump）。`APP_VERSION` は deploy.sh が自動注入。

## 段階的リリース案

| Phase | 内容 | 成果 |
|-------|------|------|
| **0** | Q65 の end-to-end 動作検証（WAV + ライブ） | 既存機能の健全性確認 |
| **1** | A+B+C+D — WAV ドロップでの FST4 デコード | 全5サブモード即動作、ライブは FST4-15 のみ |
| **2** | E — スナップショットバッファ可変長化 | FST4-30/60/120/300 ＋ Q65-60/WSPR ライブ対応 |
| **3** | FST4 エンコード/TX + sniper モード | 送信・スナイパー統合 |

## リスク / 確認事項
- FST4W は mfsk-core 未実装 → 別タスク（upstream 対応要）。
- FST4 SIC / wide-band AP 未実装 → subtract 省略、AP は sniper のみ。
- 長周期バッファのメモリ（14.4MB）とモバイル端末での挙動。
- 周波数レンジ・`sync_min`・`max_cand` の実測チューニング（`ft8-bench` で検証可能）。
