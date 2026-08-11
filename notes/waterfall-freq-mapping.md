# ウォーターフォール — スペクトラムとオーバーレイで pixel→Hz 写像が違う

> 発見日: 2026-08-12 / 対象: `webft8` main `8876449`（v0.9.1）
> 状態: **未修正**。直し方の方針が未決（末尾の「未決事項」）

## 症状

同じキャンバス上で、スペクトラム本体とその上に載る注記が**別々の pixel→Hz 写像**を使っている。

| 描画物 | 実装 | 写像 |
|---|---|---|
| スペクトラム本体 | `waterfall.js` `_renderRow()` | `px → binMin + (px/w)·numBins → ×(sampleRate/fftSize) Hz` |
| 周波数軸の目盛 | `_drawFreqAxisInternal()` | `px → freqMin + (px/w)·(freqMax−freqMin)` |
| DF ライン | `_drawDfLine()` | 同上 |
| ターゲットライン | `_drawTargetLine()` | 同上 |
| デコード結果ラベル | `drawLabels()` | 同上 |

`binMin = floor(freqMin/bps)`、`binMax = ceil(freqMax/bps)`（`bps = sampleRate/fftSize`）と
**外側に丸めている**ため、スペクトラムが実際に覆う帯域は `freqMin..freqMax` より広い。
オーバーレイ側はそれを知らず、キャンバス幅にちょうど `freqMin..freqMax` が収まっている前提で
位置を計算する。両者は `freqMin` と `freqMax` がビン幅の整数倍のときしか一致しない。

## 実測

`freqMin=100`, `freqMax=3000`, 幅 800 px で、同じ pixel に対して両写像が返す Hz の差の最大値:

| 経路 | ビン幅 | スペクトラムの実スパン | 最大ずれ |
|---|---|---|---|
| ライブ 12 kHz / fft 2048 | 5.859 Hz | 99.6 – 3000.0 Hz | **0.4 Hz** |
| ライブ 6 kHz / fft 1024 | 5.859 Hz | 99.6 – 3000.0 Hz | **0.4 Hz** |
| WAV ドロップ @48 kHz / 2048 | 23.438 Hz | 93.8 – 3000.0 Hz | **6.3 Hz** |
| WAV ドロップ @44.1 kHz / 2048 | 21.533 Hz | 86.1 – 3014.6 Hz | **14.6 Hz** |

再現:

```bash
node -e '
const W=800, fMin=100, fMax=3000;
for (const [sr,fft] of [[12000,2048],[6000,1024],[48000,2048],[44100,2048]]) {
  const bps=sr/fft, binMin=Math.floor(fMin/sr*fft), binMax=Math.ceil(fMax/sr*fft), n=binMax-binMin;
  const fSpec=px=>(binMin+(px/W)*n)*bps, fOver=px=>fMin+(px/W)*(fMax-fMin);
  let worst=0; for(let px=0;px<=W;px++) worst=Math.max(worst,Math.abs(fSpec(px)-fOver(px)));
  console.log(sr, fft, bps.toFixed(3), fSpec(0).toFixed(1), fSpec(W).toFixed(1), worst.toFixed(1));
}'
```

## 影響

- **ライブ運用は実害なし。** 0.4 Hz は目視できず、デコードにも無関係。CLAUDE.md 3.3 の
  「12k/2048 と 6k/1024 は bin 幅完全同一」も数値上そのとおりだった（両方 5.859 Hz、
  丸め後のビン範囲も一致）
- **問題は WAV ドロップ経路。** `app.js` の `handleFile()` は
  `waterfall.setSampleRate(wavRate)` で WAV の実レートに切り替えるが `fftSize` は変えない。
  44.1 kHz の録音では DF ラインとデコードラベルが実際の信号から最大 **14.6 Hz** ずれる。
  FT8 のトーン間隔 6.25 Hz の 2 倍以上
- 目視では「なんとなくずれている」以上のことは分からない。シミュレータ WAV の目視検証に
  この経路を使うと判断を誤らせる

## 未決事項（次のセッションで決める）

どちらの写像を真とするか。

- **案 A（推奨）**: `hzToX(hz)` / `xToHz(x)` を 1 組だけ定義し、**ビン写像から導出**して
  スペクトラムもオーバーレイも同じものを使う。スペクトラムがデータで、オーバーレイはそれを
  説明するものだから、真とすべきはビン側。副作用として軸目盛の位置が僅かに動く
- **案 B**: `binMin`/`binMax` を丸めて `freqMin`/`freqMax` にぴったり合わせ、表示範囲を厳密に
  100–3000 Hz にする。描画されるスペクトラムの端が僅かに変わる

## テストの順序

先に「オーバーレイが主張する Hz == スペクトラムがそこに描いている Hz」を全対応サンプルレート
について表明するテストを書き、**赤を確認してから**直す
（`notes/tx-test-strategy.md` の「新しいテストは修正前のコードに対して落ちることを確認してから
採用する」に従う）。`Waterfall` はコンストラクタで `canvas.getContext('2d')` と
`document.createElement` を呼ぶので、node から回すにはキャンバスのスタブが要る。
