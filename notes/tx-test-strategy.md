# UI 側テスト戦略 — 状態機械を引き剥がして、音と PTT を実測する

> 策定日: 2026-08-12 / 対象: `webft8` main `57a8ed9`（v0.9.1）
> 前提文書: `notes/tx-safety-invariants.md`（何を守るのかはそちら）

## 現状（実測）

| 項目 | 実測値 |
|---|---|
| CI | **存在しない**（`.github/` 自体が無い） |
| JS テストフレームワーク | 無し |
| JS 検証スクリプト | `notes/*.mjs` 3本、すべて手動実行 |
| Rust テスト | `ft8-bench` 3ファイル、`uvpacket-web` 4ファイル。`ft8-web/src/lib.rs` は 0 |
| ブラウザ側 JS | 5509 行。うち `app.js` が **2730 行（約半分）** |
| `app.js` の `export` | **0** |
| `app.js` のモジュールスコープ `getElementById` | **122 箇所** |

## 問題の所在

「UI テストが無い」ではなく、**`app.js` が構造的にテスト不能**なことが問題である。
`export` が 0 なので import できず、テストの書きようがない。一方で他の 12 モジュールは
すべて `export` を持っており、今日でも書けるのに 1 本しか無い。

v0.9.1 の不具合はその境目に落ちた。追加した `audio-output` のテストは
[不変条件](tx-safety-invariants.md) 7 しか守っておらず、**報告された症状そのもの
（停止経路が TX を落とさない = 条件 1〜6）は現時点で無防備**である。

方針は「UI の描画をテストする」ではなく、**UI から状態機械を引き剥がして単体で固定し、
その上で音と PTT を実測で押さえる**。ウォーターフォールの描画や `tx-active` クラスの
付け外しは、壊れれば目で見えるのでテストしない。

## Phase 1 — 状態機械を `app.js` から引き剥がす

**新規** `ft8-web/www/tx-session.js` / **改修** `ft8-web/www/app.js`

DOM は `app.js` に残し、TX ライフサイクルだけを注入型モジュールに出す。全面リファクタは
しない（移設対象は `txActive` 13 箇所 / `halted` 18 箇所 / `liveMode` 10 箇所）。

```js
// tx-session.js — DOM を一切知らない
export class TxSession {
  constructor({ periodMgr, audioOut, cat, indicators })
  async stopLive()   // 停止経路 A / C / D
  async halt()       // 停止経路 B
  async abortTx()    // 上の共通の後始末（現 app.js の abortTx を移設）
  get txActive() / get halted() / get txQueued()
}
```

- `indicators` は `{ setTxOn, setTxMeter, clearTxActive, setHaltGlyph, setToneLabel }` 程度の
  薄いオブジェクト。`app.js` が DOM 実装を、テストが記録用スタブを渡す
- 既存の `cat.safePttOff()` と `periodMgr.cancelTx()` を再利用する。新規実装しない

**新規** `tests/unit/tx-session.test.mjs`（`node:test`。vitest / jest は入れない）

停止経路 A〜D × 不変条件 3〜6 のテーブル駆動。`periodMgr` / `audioOut` / `cat` はスタブ。

**移設** `notes/audio-output-stop-check.mjs` → `tests/unit/audio-output.test.mjs`
（`notes/wspr-decode-baseline.mjs` は測定記録なので `notes/` に残す）

**新規** `tests/unit/stop-paths.test.mjs` — 「5 つ目の停止経路」対策。`app.js` を文字列として
読み、`capture.stop()` を呼ぶ箇所がすべて `TxSession.stopLive()` を経由していることを表明する。
lint 相当の粗い検査だが、v0.9.1 の抜けを機械的に捕まえられる唯一の安価な手段。

## Phase 2 — CI

**新規** `.github/workflows/ci.yml`

| job | 内容 |
|---|---|
| `js` | `node --test tests/unit/`、`ft8-web/www/*.js` 全ファイルに `node --check` |
| `rust` | `cargo test -p ft8-bench -p uvpacket-web` |

`ft8-desktop/src-tauri` はワークスペース除外かつ Linux で webkit 依存が要るので対象外。

## Phase 3 — 受け入れテスト（音と PTT の実測）

**新規** `tests/e2e/`（独自 `package.json`。ルートには置かない）

対象は Web 版。Playwright で Chromium を**永続プロファイル**起動する。

### 成立の根拠（調査済み）

- Web Serial の許可は永続化され、`app.js` は Start Audio 時に `navigator.serial.getPorts()` が
  **1 本だけなら自動接続**する。よって**一度だけ手で許可**すれば、以後チューザ無しで無人実行できる
- 出力デバイスは `outputDeviceSelect` で選べ、`AudioOutput.play()` は `setSinkId` を通す。
  よって VB-CABLE への振り分け経路そのものも検証対象になる
- 参考: `cat.js` の Tauri 経路は `serial_open` にポート名を渡すのでチューザが無い。将来
  デスクトップ版で同じ試験をするならこちらの方が素直

### 構成

```
COM3 (app が開く)  ──com0com──  COM4 (Node が読む)   → PTT バイト列
出力→ CABLE Input  ──VB-CABLE──  CABLE Output        → 別ページの getUserMedia で RMS
```

### シナリオ `tests/e2e/stop-kills-tx.spec.mjs`

1. 永続プロファイルで起動、localStorage に呼出符号・グリッド・リグ機種を投入
2. Start Audio → CAT 自動接続を待つ
3. Test Tone を開始（TX 経路を確定的に励起できる。実 QSO 不要）
4. RMS > 閾値、COM4 に PTT on バイト列を確認
5. **Stop Audio をクリック**
6. 200 ms 以内に RMS がノイズフロアへ落ちること、COM4 に **PTT off バイト列**が届くことを表明
7. 同じ表明を Halt 経路でも繰り返す

### 一度だけ必要な導入

- **VB-CABLE**（donationware・無償）
- **com0com**（GPL・無償）

いずれも有料ではない。Stereo Mix によるゼロインストール代替は**この PC では不可**
（録音デバイスはマイク 2 本のみ、Stereo Mix エンドポイント無し）を確認済み。

## 検証手順

```bash
node --test tests/unit/          # Phase 1
cd tests/e2e && npm test         # Phase 3（仮想デバイス導入後）
```

**新しいテストは、修正前のコードに対して実際に落ちることを確認してから採用する。**
v0.9.1 では `audio-output-stop-check.mjs` を修正前ファイルに対して走らせ、
`FAIL stop() settles play()` と `TypeError: Cannot read properties of null (reading 'close')`
が出ることを確認した。落ちないテストは、通っていても何も守っていない。

## リスクと退避

- **com0com が Windows 11 のドライバ署名に弾かれる可能性**が最大の不確実性。その場合は
  Phase 3 の PTT 部分を「`Cat` に記録用トランスポートを注入して送出バイト列を検証する」に
  縮退させる。線には出ないがアプリのロジックは押さえられる。音の実測は VB-CABLE のみで
  独立に成立するので影響しない
- Phase 1 は `app.js` に触れる。**v0.9.1 の実機確認が済んでから**着手するのが安全

## 非目標

- ウォーターフォール描画や `tx-active` クラス付け外しの検証 — 壊れれば目で見える
- vitest / jest / Testing Library の導入 — `node:test` で足り、依存を増やさない
- Tauri デスクトップ版の E2E — 同じ不変条件は適用できるが、tauri-driver のセットアップは範囲外
