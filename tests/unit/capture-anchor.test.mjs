// SPDX-License-Identifier: GPL-3.0-or-later
//
// AudioCapture's UTC↔audio-frame anchor and the window it derives from it
// (issue #11, defect 1).
//
//   node --test tests/unit/
//
// Nothing here touches Web Audio: the anchor is fed the same (frame, arrival)
// pairs the worklet's peak report delivers, and the worklet node is a stub that
// records what was asked for. Against the pre-fix file the import itself still
// works but `snapshotSlot` does not exist — `snapshot()` took no window,
// because there was none to take.

import { test } from 'node:test';
import assert from 'node:assert/strict';

const { AudioCapture } = await import(
  new URL('../../ft8-web/www/audio-capture.js', import.meta.url).href
);

const RATE = 12000;
const realNow = Date.now;

/** An AudioCapture wired to a stub port, with `Date.now()` under our control. */
function makeCapture() {
  const cap = new AudioCapture({});
  cap.actualSampleRate = RATE;
  const asked = [];
  cap.workletNode = { port: { postMessage: (m) => asked.push(m) } };
  return { cap, asked };
}

/** Deliver a peak report for `frame` that took `delayMs` to arrive at `atMs`. */
function deliver(cap, frame, atMs) {
  Date.now = () => atMs;
  cap._noteFrame(frame);
}

test('the epoch estimate is the minimum candidate, not the latest', async () => {
  const { cap } = makeCapture();
  const EPOCH = 1_700_000_000_000;

  // frame 12000 = 1.000 s of audio. A report that took 30 ms to arrive says
  // the epoch is 30 ms later than it is; one that took 2 ms says 2 ms later.
  deliver(cap, RATE * 1, EPOCH + 1000 + 30);
  assert.equal(cap.frameEpochMs, EPOCH + 30);

  deliver(cap, RATE * 2, EPOCH + 2000 + 2);
  assert.equal(cap.frameEpochMs, EPOCH + 2, 'a quicker report did not tighten the estimate');

  // A slow one afterwards must not drag it back out — delivery delay is
  // one-sided, so only the minimum carries information.
  deliver(cap, RATE * 3, EPOCH + 3000 + 80);
  assert.equal(cap.frameEpochMs, EPOCH + 2);
  assert.equal(cap.epochSpreadMs, 78);

  Date.now = realNow;
});

test('the anchor window slides so a session-long drift is followed', async () => {
  const { cap } = makeCapture();
  cap._EPOCH_WINDOW = 3;
  const EPOCH = 1_700_000_000_000;

  deliver(cap, RATE * 1, EPOCH + 1000 + 1);   // the tight one
  deliver(cap, RATE * 2, EPOCH + 2000 + 40);
  deliver(cap, RATE * 3, EPOCH + 3000 + 40);
  assert.equal(cap.frameEpochMs, EPOCH + 1);

  // Once it falls out of the window the estimate must let go of it, or a
  // single lucky early sample would pin the mapping for the whole session
  // while the audio clock drifts away from the system clock.
  deliver(cap, RATE * 4, EPOCH + 4000 + 40);
  assert.equal(cap.frameEpochMs, EPOCH + 40);

  Date.now = realNow;
});

test('a slot is requested by its place on the clock', async () => {
  const { cap, asked } = makeCapture();
  const EPOCH = 1_700_000_000_000;
  deliver(cap, RATE * 1, EPOCH + 1000);        // exact, zero delay
  assert.equal(cap.frameEpochMs, EPOCH);

  Date.now = () => EPOCH + 45_000;
  cap.snapshotSlot(EPOCH + 30_000, 15_000);    // the slot 30-45 s after frame 0
  Date.now = realNow;

  const req = asked.at(-1);
  assert.equal(req.type, 'snapshot');
  assert.equal(req.fromFrame, 30 * RATE);
  assert.equal(req.nFrames, 15 * RATE);
});

test('asking late does not move the window', async () => {
  // The whole point of the anchor. Under fill-and-rewind the returned span
  // started wherever the previous request happened to be serviced, so a
  // main thread 40 ms behind shifted every sample by 40 ms.
  const { cap, asked } = makeCapture();
  const EPOCH = 1_700_000_000_000;
  deliver(cap, RATE, EPOCH + 1000);

  Date.now = () => EPOCH + 45_000;
  cap.snapshotSlot(EPOCH + 30_000, 15_000);
  Date.now = () => EPOCH + 45_000 + 40;        // 40 ms of main-thread jank
  cap.snapshotSlot(EPOCH + 30_000, 15_000);
  Date.now = realNow;

  const win = (m) => ({ fromFrame: m.fromFrame, nFrames: m.nFrames });
  assert.deepEqual(win(asked.at(-1)), win(asked.at(-2)));
});

test('overlapping requests are answered to the caller that made them', async () => {
  // The period loop no longer serialises the boundary handlers, so two can be
  // waiting on the worklet at once. A single pending-resolve slot would have
  // the second orphan the first, which then sat until its 5 s timeout.
  const { cap, asked } = makeCapture();
  const first = cap.snapshotSlot(null, 15_000);
  const second = cap.snapshotSlot(null, 7_500);
  const [idA, idB] = asked.slice(-2).map((m) => m.id);
  assert.notEqual(idA, idB, 'both requests carried the same id');

  // The real dispatch happens in the port handler `start()` installs; here the
  // pending resolver is called straight, which is the same lookup.
  const reply = (id, tag) => cap._snapPending.get(id)({
    id, samples: new Float32Array([tag]), sampleRate: RATE, missingHead: 0, missingTail: 0,
  });
  reply(idB, 2);                       // out of order, as a worklet may answer
  reply(idA, 1);

  assert.equal((await first).samples[0], 1);
  assert.equal((await second).samples[0], 2);
});

test('with no anchor yet the window is null and the worklet serves the newest', async () => {
  // The first ~100 ms of a session, before any frame-stamped message has
  // landed. `fromFrame: null` is the worklet's "newest nFrames" request —
  // exactly what the old buffer did on every slot.
  const { cap, asked } = makeCapture();
  assert.equal(cap.frameEpochMs, null);
  cap.snapshotSlot(Date.now(), 15_000);
  assert.equal(asked.at(-1).fromFrame, null);
  assert.equal(asked.at(-1).nFrames, 15 * RATE);
});

test('stop() drops the anchor so the next context does not inherit it', async () => {
  const { cap } = makeCapture();
  deliver(cap, RATE, 1_700_000_001_000);
  Date.now = realNow;
  assert.ok(cap.frameEpochMs != null);

  cap.running = true;                          // stop() is a no-op otherwise
  cap.stop();
  assert.equal(cap.frameEpochMs, null, 'frame numbering from a closed context survived');
});

test('a worklet that never answers still settles the promise', async () => {
  // The period loop no longer waits on this, but a permanently pending
  // promise would still leak a slot's worth of state on every boundary.
  const cap = new AudioCapture({});
  cap.actualSampleRate = RATE;
  cap.workletNode = null;                      // capture already torn down
  const r = await cap.snapshotSlot(Date.now(), 15_000);
  assert.equal(r.samples.length, 0);
});
