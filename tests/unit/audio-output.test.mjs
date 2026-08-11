// SPDX-License-Identifier: GPL-3.0-or-later
//
// Stubs just enough Web Audio to exercise AudioOutput's play/stop lifecycle.
//
//   node --test tests/unit/
//
// The bug under test (fixed in v0.9.1): stop() left play()'s promise
// unsettled, so the awaiting transmit() never resumed — PTT was never dropped
// and txActive stayed true forever. Run this against the pre-fix file
// (`git show 57a8ed9~1:ft8-web/www/audio-output.js`) and it fails
// "stop() settles play()" and then throws
// `TypeError: Cannot read properties of null (reading 'close')` from the late
// onended. That failure is the signature of the bug — a test that cannot
// produce it is not covering anything.
//
// Covers invariant 7 of notes/tx-safety-invariants.md.

import { test } from 'node:test';
import assert from 'node:assert/strict';

let closedCount = 0;

class FakeParam { constructor() { this.value = 0; } }
class FakeNode {
  constructor(ctx) { this.ctx = ctx; this.onended = null; this.started = false; this.stopped = false; }
  connect() {}
  start() { this.started = true; }
  stop() { this.stopped = true; }
  /** Simulate the buffer reaching its natural end. */
  fireEnded() { if (this.onended) this.onended(); }
}
class FakeBufferSource extends FakeNode { constructor(ctx) { super(ctx); this.buffer = null; } }
class FakeOscillator extends FakeNode {
  constructor(ctx) { super(ctx); this.type = ''; this.frequency = new FakeParam(); }
}
class FakeGain extends FakeNode { constructor(ctx) { super(ctx); this.gain = new FakeParam(); } }

globalThis.AudioContext = class {
  constructor() { this.state = 'running'; this.destination = {}; this.closed = false; this.lastSource = null; }
  async resume() { this.state = 'running'; }
  createBuffer() { return { copyToChannel() {} }; }
  createBufferSource() { this.lastSource = new FakeBufferSource(this); return this.lastSource; }
  createOscillator() { this.lastSource = new FakeOscillator(this); return this.lastSource; }
  createGain() { return new FakeGain(this); }
  close() { this.closed = true; closedCount++; }
};

const { AudioOutput } = await import(
  new URL('../../ft8-web/www/audio-output.js', import.meta.url).href
);

const samples = new Float32Array(12000 * 13);

/** Let play() get past its `await resume()` / `await setSinkId()`. */
const settleSetup = async () => { await null; await null; };

/** Resolve to 'HUNG' rather than waiting forever on an unsettled promise. */
const orHang = (p, ms = 50) =>
  Promise.race([p, new Promise((r) => setTimeout(() => r('HUNG'), ms))]);

test('a burst that reaches its end resolves true and closes the context', async () => {
  const out = new AudioOutput();
  const p = out.play(samples);
  await settleSetup();
  const ctx = out.ctx;
  ctx.lastSource.fireEnded();
  assert.equal(await p, true);
  assert.equal(ctx.closed, true);
  assert.equal(out.playing, false);
});

test('stop() mid-burst settles play() with false', async () => {
  const out = new AudioOutput();
  const p = out.play(samples);
  await settleSetup();
  const ctx = out.ctx;
  const src = ctx.lastSource;

  assert.equal(await Promise.race([p, Promise.resolve('PENDING')]), 'PENDING',
    'play() should still be pending while the burst runs');

  out.stop();
  const completed = await orHang(p);
  assert.notEqual(completed, 'HUNG', 'stop() must settle play(), not hang it');
  assert.equal(completed, false);
  assert.equal(src.stopped, true);
  assert.equal(ctx.closed, true);
  assert.equal(out.playing, false);

  // A late onended must not re-settle the promise as "completed".
  src.fireEnded();
  assert.equal(src.onended, null);
});

test('stop() during play()\'s own setup await still settles it', async () => {
  const out = new AudioOutput();
  const p = out.play(samples);
  out.stop();                     // abort before the graph exists
  const completed = await orHang(p);
  assert.notEqual(completed, 'HUNG');
  assert.equal(completed, false);
});

test('a second play() supersedes the first', async () => {
  const out = new AudioOutput();
  const p1 = out.play(samples);
  const p2 = out.play(samples);
  await settleSetup();
  assert.equal(await orHang(p1), false, 'the superseded burst resolves false');
  out.ctx.lastSource.fireEnded();
  assert.equal(await p2, true, 'the newer burst still completes');
});

test('stop() tears down a test tone', async () => {
  const out = new AudioOutput();
  await out.startTone(1500);
  assert.equal(out.playing, true);
  const ctx = out.ctx;
  out.stop();
  assert.equal(out.playing, false);
  assert.equal(ctx.closed, true);
});

test('stop() on an idle output is a no-op', () => {
  const out = new AudioOutput();
  const before = closedCount;
  out.stop();
  out.stop();
  assert.equal(closedCount, before);
});
