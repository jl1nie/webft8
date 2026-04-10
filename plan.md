# Plan: Web Serial ポートロック修正 + BLE トランスポート追加

## 背景

`cat.js` の Web Serial 実装に致命的なバグがあり、ポートが恒久的にロックされる。
Windows では特にシリアルポートの排他制御が厳格なため、一度ロック状態に陥ると
ブラウザ再起動まで復旧できない。

## 根本原因

`cat.js:68`:
```javascript
this.port.readable.pipeTo(new WritableStream()).catch(() => this._handleDisconnect());
```

- `pipeTo()` が readable stream にリーダーロックを取得
- ロック参照が未保存 → キャンセル不可能
- `disconnect()` で writer のみ `releaseLock()` → reader ロック残存
- `port.close()` は reader ロックが残っていると失敗する（仕様通り）

## 修正箇所

### 1. `cat.js` — readable ストリーム処理の全面修正

**変更前** (connect):
```javascript
if (this.port.readable) {
  this.port.readable.pipeTo(new WritableStream()).catch(() => this._handleDisconnect());
}
```

**変更後**: `pipeTo()` を廃止し、`getReader()` + 明示的 read loop に変更。
reader 参照を `this._reader` に保存し、disconnect 時に確実にキャンセルする。

```javascript
constructor() {
  // ... 既存フィールド ...
  this._reader = null;
  this._readLoopDone = null;  // read loop の完了を待つための Promise
}

async connect(rigId) {
  if (!this.port) throw new Error('No port selected');
  const rig = rigProfiles[rigId];
  if (!rig) throw new Error(`Unknown rig: ${rigId}`);

  this.rig = rig;
  this.rigId = rigId;

  await this.port.open({ baudRate: rig.baud });

  // writer 取得
  this.writer = this.port.writable.getWriter();

  // reader 取得 + read loop 開始
  if (this.port.readable) {
    this._reader = this.port.readable.getReader();
    this._readLoopDone = this._readLoop();
  }

  this.connected = true;
  this.pttOn = false;
  this.narrowOn = false;
}
```

**新規: `_readLoop()` メソッド**:
```javascript
async _readLoop() {
  try {
    while (true) {
      const { done } = await this._reader.read();
      if (done) break;
      // CI-V 応答の処理は将来拡張ポイント
    }
  } catch (_) {
    // ポート切断やキャンセル時にここに来る
  } finally {
    // reader ロックを確実に解放
    try { this._reader.releaseLock(); } catch (_) {}
    this._reader = null;
  }
}
```

### 2. `cat.js` — disconnect の全面修正

**変更前**:
```javascript
async disconnect() {
  await this.safePttOff();
  if (this.writer) { this.writer.releaseLock(); this.writer = null; }
  try { if (this.port) await this.port.close(); } catch (_) {}
  this.connected = false;
  this.pttOn = false;
  this.narrowOn = false;
}
```

**変更後**: reader → writer → port の順に確実に解放。
```javascript
async disconnect() {
  this.connected = false;
  await this.safePttOff();

  // 1. reader をキャンセル（read loop を終了させる）
  if (this._reader) {
    try { await this._reader.cancel(); } catch (_) {}
  }
  // read loop の完了を待つ（releaseLock はここで実行される）
  if (this._readLoopDone) {
    await this._readLoopDone;
    this._readLoopDone = null;
  }

  // 2. writer ロック解放
  if (this.writer) {
    try { this.writer.releaseLock(); } catch (_) {}
    this.writer = null;
  }

  // 3. ポートを閉じる（全ロック解放後なので安全）
  try { if (this.port) await this.port.close(); } catch (_) {}

  this.pttOn = false;
  this.narrowOn = false;
}
```

### 3. `cat.js` — _handleDisconnect にもクリーンアップ追加

`_handleDisconnect()` はエラー時のコールバック経路で呼ばれるため、
ここでもリソース解放を行う。

```javascript
_handleDisconnect() {
  this.connected = false;
  this.pttOn = false;
  this.narrowOn = false;

  // reader キャンセル（非同期だがベストエフォート）
  if (this._reader) {
    try { this._reader.cancel(); } catch (_) {}
  }
  // writer ロック解放
  if (this.writer) {
    try { this.writer.releaseLock(); } catch (_) {}
    this.writer = null;
  }

  if (this.onDisconnect) this.onDisconnect();
}
```

### 4. `cat.js` — connect 失敗時のロールバック

`connect()` が途中で例外を投げた場合、既に開いたポートや取得したロックを
確実に解放する。

```javascript
async connect(rigId) {
  // ... validation ...
  try {
    await this.port.open({ baudRate: rig.baud });
    this.writer = this.port.writable.getWriter();
    if (this.port.readable) {
      this._reader = this.port.readable.getReader();
      this._readLoopDone = this._readLoop();
    }
    this.connected = true;
    this.pttOn = false;
    this.narrowOn = false;
  } catch (e) {
    // ロールバック: 取得済みリソースを全解放
    await this.disconnect();
    throw e;
  }
}
```

### 5. `app.js` — connect エラー時のクリーンアップ

```javascript
// 現在 (line 936-938):
} catch (e) {
  catStatusEl.textContent = `error: ${e.message || e}`;
}
```

エラー発生時に `cat.disconnect()` を呼んで確実にクリーンアップ:
```javascript
} catch (e) {
  await cat.disconnect();
  catStatusEl.textContent = `error: ${e.message || e}`;
}
```

## 修正対象ファイル一覧

| ファイル | 変更内容 |
|----------|----------|
| `ft8-web/www/cat.js` | readable ストリーム処理、disconnect、エラーリカバリ全面修正 |
| `ft8-web/www/app.js` | connect エラー時のクリーンアップ追加 |

## 修正しないもの

- BLE トランスポート追加は別タスク（本修正が前提）
- rig-profiles.json の拡充は別タスク
- CI-V レスポンス解析（将来拡張ポイントとして read loop にコメントを残す）
