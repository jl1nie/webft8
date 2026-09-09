// SPDX-License-Identifier: GPL-3.0-or-later
//
// The capture path in audio-processor.js: a ring addressed by absolute audio
// frame, so the window handed to the decoder is chosen on the clock instead of
// being "whatever arrived since the last snapshot message was serviced"
// (issue #11, defect 1).
//
//   node --test tests/unit/
//
// Run against the pre-fix file (`git show HEAD:ft8-web/www/audio-processor.js`)
// and every test here fails at the first `snapshot` message: the old worklet
// took no frame range, answered with `buffer.slice(0, writePos)`, and reset
// `writePos` as a side effect of being read.
//
// The worklet runs in AudioWorkletGlobalScope, which is a module scope with no
// imports and a handful of magic globals. Stub those, capture the class out of
// `registerProcessor`, and it is ordinary JS.

import { test } from 'node:test';
import assert from 'node:assert/strict';

const RATE = 12000;
const QUANTUM = 128;

let frameCounter = 0;
Object.defineProperty(globalThis, 'currentFrame', { get: () => frameCounter, configurable: true });
globalThis.sampleRate = RATE;

let registered = null;
globalThis.registerProcessor = (name, cls) => { registered = cls; };

/** The port half the worklet writes to; collects what it posts. */
class FakePort {
  constructor() { this.onmessage = null; this.posted = []; }
  postMessage(msg) { this.posted.push(msg); }
  /** Deliver a message to the worklet, as the main thread would. */
  send(data) { this.onmessage({ data }); }
  /** Most recent message of a type, or undefined. */
  last(type) { return [...this.posted].reverse().find((m) => m.type === type); }
}

globalThis.AudioWorkletProcessor = class {
  constructor() { this.port = new FakePort(); }
};

await import(new URL('../../ft8-web/www/audio-processor.js', import.meta.url).href);
assert.ok(registered, 'audio-processor.js did not register a processor');

/**
 * A worklet started at `startFrame`, plus a `feed(n)` that renders `n` frames
 * of a ramp whose value *is* the absolute frame index — so any sample the ring
 * returns identifies the frame it came from, and a window can be checked
 * exactly rather than approximately.
 */
function makeWorklet({ startFrame = 4_000_000, seconds = 4 } = {}) {
  frameCounter = startFrame;
  const w = new registered({ processorOptions: {} });
  w.port.send({ type: 'setBufferSeconds', seconds });
  w.port.send({ type: 'start' });

  // Renders whole quanta, so it overshoots `frames` by up to 127; returns what
  // it actually wrote for the tests that care to the sample.
  const feed = (frames) => {
    let done = 0;
    while (done < frames) {
      const block = new Float32Array(QUANTUM);
      for (let i = 0; i < QUANTUM; i++) block[i] = frameCounter + i;
      w.process([[block]]);
      frameCounter += QUANTUM;
      done += QUANTUM;
    }
    return done;
  };

  const request = (fromFrame, nFrames) => {
    w.port.send({ type: 'snapshot', fromFrame, nFrames });
    return w.port.last('snapshot');
  };

  return { w, feed, request };
}

test('a requested frame range comes back as exactly that range', async () => {
  const { feed, request } = makeWorklet();
  const base = frameCounter;
  feed(RATE * 3);

  const from = base + RATE;          // one second in
  const n = RATE;                    // one second long
  const r = request(from, n);

  assert.equal(r.samples.length, n);
  assert.equal(r.missingHead, 0);
  assert.equal(r.missingTail, 0);
  assert.equal(r.samples[0], from, 'window did not start at the frame asked for');
  assert.equal(r.samples[n - 1], from + n - 1, 'window did not end where it should');
});

test('the window does not move when the request arrives late', async () => {
  // The defect this whole change exists for: under fill-and-rewind, the phase
  // of the window was set by when the message happened to be serviced. Here,
  // half a second of extra audio between asking early and asking late must
  // make no difference at all to the samples returned.
  const { feed, request } = makeWorklet();
  const base = frameCounter;
  feed(RATE * 2);
  const early = request(base + RATE / 2, RATE);
  feed(RATE / 2);                    // the main thread was busy
  const late = request(base + RATE / 2, RATE);

  assert.deepEqual(Array.from(late.samples), Array.from(early.samples));
});

test('reading is non-destructive — the same slot answers twice', async () => {
  // The old buffer reset its write pointer as a side effect of being read, so
  // a shed or repeated slot corrupted the next one.
  const { feed, request } = makeWorklet();
  const base = frameCounter;
  feed(RATE * 2);
  const a = request(base + 100, RATE);
  const b = request(base + 100, RATE);
  assert.deepEqual(Array.from(b.samples), Array.from(a.samples));

  // And audio kept arriving in between, at the right frames.
  feed(RATE);
  const c = request(base + RATE * 2, RATE);
  assert.equal(c.samples[0], base + RATE * 2);
  assert.equal(c.missingTail, 0);
});

test('a window that has rolled off the ring is padded and declared short', async () => {
  const { feed, request } = makeWorklet({ seconds: 2 });
  const base = frameCounter;
  feed(RATE * 4);                    // twice round a 2 s ring

  const r = request(base, RATE);     // long gone
  assert.equal(r.samples.length, RATE);
  assert.equal(r.missingHead, RATE, 'stale window was served as if it were real audio');
  assert.ok(r.samples.every((v) => v === 0));
});

test('a window that runs past the newest frame is padded at the tail', async () => {
  const { feed, request } = makeWorklet();
  const base = frameCounter;
  const written = feed(RATE);
  const r = request(base, RATE * 2);   // asks for a second that has not happened

  assert.equal(r.missingHead, 0);
  assert.equal(r.missingTail, RATE * 2 - written);
  assert.equal(r.samples[0], base);
  assert.equal(r.samples[written - 1], base + written - 1);
  assert.equal(r.samples[written], 0, 'audio continued past the newest frame');
  assert.equal(r.samples[RATE * 2 - 1], 0);
});

test('the ring wraps without seams', async () => {
  const { feed, request } = makeWorklet({ seconds: 2 });
  const base = frameCounter;
  feed(RATE * 3);                     // write head is 1 s past the wrap

  // A window straddling the wrap point must still be contiguous frames.
  const from = base + RATE * 3 - RATE;
  const r = request(from, RATE);
  assert.equal(r.missingHead, 0);
  assert.equal(r.missingTail, 0);
  for (let i = 0; i < RATE; i++) {
    assert.equal(r.samples[i], from + i, `discontinuity at offset ${i}`);
  }
});

test('frames the worklet never saw read back as silence, not as last lap', async () => {
  // Capture stopped and restarted: `currentFrame` ran on without us. Those
  // frames must not return whatever the ring held there a lap ago.
  const { w, feed, request } = makeWorklet({ seconds: 4 });
  const base = frameCounter;
  feed(RATE * 2);

  w.port.send({ type: 'stop' });
  frameCounter += RATE;              // a second of audio goes unrecorded
  w.port.send({ type: 'start' });    // start() resets, as AudioCapture does
  const resume = frameCounter;
  feed(RATE);

  const r = request(resume, RATE);
  assert.equal(r.missingHead, 0);
  assert.equal(r.samples[0], resume);

  // The unrecorded second is behind `firstFrame`, so it reads as missing
  // rather than as the audio that occupied those ring slots before.
  const gap = request(base + RATE, RATE);
  assert.ok(gap.missingHead > 0, 'unrecorded frames were served as audio');
});

test('the ring follows the frame counter, not a fixed block size', async () => {
  // A render quantum is 128 frames and nothing in the platform suggests it
  // will change, but the write position is taken from `currentFrame` each
  // block rather than accumulated, so that assumption is not load-bearing:
  // blocks of any length land at the frame they claim.
  const { w, request } = makeWorklet();
  const base = frameCounter;
  for (const n of [64, 128, 256, 100, 512]) {
    const block = new Float32Array(n);
    for (let i = 0; i < n; i++) block[i] = frameCounter + i;
    w.process([[block]]);
    frameCounter += n;
  }
  const total = 64 + 128 + 256 + 100 + 512;
  const r = request(base, total);
  assert.equal(r.missingHead, 0);
  assert.equal(r.missingTail, 0);
  for (let i = 0; i < total; i++) {
    assert.equal(r.samples[i], base + i, `frame numbering broke at offset ${i}`);
  }
});

test('the peak report carries the frame counter for the clock anchor', async () => {
  const { w, feed } = makeWorklet();
  feed(RATE);
  const peak = w.port.last('peak');
  assert.ok(peak, 'no peak report');
  assert.equal(typeof peak.frame, 'number');
  // Stamped at the end of the block that posted it, so it never runs ahead of
  // the frames the main thread could already have.
  assert.ok(peak.frame <= frameCounter, 'peak frame stamp is in the future');
  assert.ok(peak.frame > 0);
});

test('the peak report goes out about ten times a second', async () => {
  // The interval counts frames, and it used to be set to the number of
  // *quanta* in 100 ms (nine) — which a single 128-frame block already clears,
  // so the message went out every quantum: ~94 a second, and a peak measured
  // over 10.7 ms rather than the 100 ms the meter is scaled for. It is also
  // the clock anchor's sample, and AudioCapture sizes its estimation window in
  // these reports, so the cadence has to be what it says it is.
  const { w, feed } = makeWorklet();
  feed(RATE);
  const peaks = w.port.posted.filter((m) => m.type === 'peak').length;
  assert.ok(peaks >= 9 && peaks <= 11, `${peaks} peak reports in one second`);
});
