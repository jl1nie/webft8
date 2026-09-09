// SPDX-License-Identifier: GPL-3.0-or-later
//
// Guards the integrity of the slot-boundary timer in ft8-period.js: one boundary
// per period, no matter what happens during the awaited decode.
//
//   node --test tests/unit/
//
// The bug these cover: the fired `setTimeout` handle was never cleared, so
// `boundaryTimeout` stayed truthy while the handler ran. `setClockOffset()`,
// which app.js calls from *inside* the awaited `onPeriodEnd` (`applyBootstrap`
// on a cold start, and the NTP/GPS handlers), saw that stale handle, did a
// no-op `clearTimeout` on it, and armed a second boundary timer — which the
// handler's own trailing `_scheduleBoundary()` then orphaned rather than
// replaced. Two independent boundary loops per slot: double decode, double
// `onPeriodStart` (hence a double WSPR beacon schedule), and a timer that
// `stop()` no longer held a reference to. It measured 25 boundaries where 10
// were due.
//
// These run on a 200 ms slot so ten of them take two seconds. The manager
// aligns to `Date.now()` and has no injectable clock, so timings are real.

import { test } from 'node:test';
import assert from 'node:assert/strict';

const { FT8PeriodManager } = await import(
  new URL('../../ft8-web/www/ft8-period.js', import.meta.url).href
);

const SLOT = 200;
const SLOTS = 10;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * Run the manager for about `SLOTS` slots, calling `duringDecode(mgr)` inside
 * the first `onPeriodEnd`, and report the period index of every boundary.
 *
 * The assertion is on the *indices*, not on a count: the run starts at an
 * arbitrary phase within a slot, so a fixed window legitimately spans either
 * `SLOTS` or `SLOTS + 1` boundaries, and counting alone is flaky. Duplicate
 * loops and early re-entry both show up as a repeated index, which is exact.
 */
async function run(duringDecode) {
  const startIdx = [];
  const endIdx = [];
  let fired = false;

  const mgr = new FT8PeriodManager({
    onPeriodStart: (i) => { startIdx.push(i); },
    onPeriodEnd: async (i) => {
      endIdx.push(i);
      if (!fired && duringDecode) {
        fired = true;
        await sleep(20);          // decode takes a moment
        await duringDecode(mgr);
      }
    },
  }, SLOT);

  mgr.start();
  await sleep(SLOT * SLOTS + 50);
  const beforeStop = startIdx.length;
  mgr.stop();
  await sleep(SLOT * 3);
  return { startIdx, endIdx, leaked: startIdx.length > beforeStop };
}

// Each boundary must be fired for a distinct, strictly increasing period —
// one loop, advancing one slot at a time. A duplicated boundary loop repeats
// an index; an early-firing timer repeats the previous one. A boundary lost
// to host scheduling jitter shows as a gap, which is tolerated: that is the
// environment, not the code under test.
function assertOnePerPeriod({ startIdx, endIdx }) {
  assert.ok(startIdx.length > 0, 'no boundary fired at all');
  for (let i = 1; i < startIdx.length; i++) {
    assert.ok(
      startIdx[i] > startIdx[i - 1],
      `onPeriodStart went ${startIdx[i - 1]} -> ${startIdx[i]}; full run ${startIdx.join(',')}`,
    );
  }
  for (let i = 1; i < endIdx.length; i++) {
    assert.ok(
      endIdx[i] > endIdx[i - 1],
      `onPeriodEnd went ${endIdx[i - 1]} -> ${endIdx[i]}; full run ${endIdx.join(',')}`,
    );
  }
  // Sanity floor: the loop actually ran for most of the window rather than
  // stopping early or wedging.
  assert.ok(
    startIdx.length >= SLOTS - 2,
    `only ${startIdx.length} boundaries in ~${SLOTS} slots`,
  );
}

test('one boundary per period with nothing touching the clock', async () => {
  assertOnePerPeriod(await run(null));
});

test('setClockOffset() during the awaited decode does not double the boundary', async () => {
  const r = await run((mgr) => mgr.setClockOffset(0.05));
  assertOnePerPeriod(r);
  assert.equal(r.leaked, false, 'stop() left a boundary timer running');
});

test('applyBootstrap() during the awaited decode does not double the boundary', async () => {
  // The cold-start path in app.js: no confirmed decode yet, so the coarse-sync
  // DT estimate seeds the offset from inside onPeriodEnd. Reaches
  // setClockOffset() through applyBootstrap()'s guards.
  const r = await run((mgr) => mgr.applyBootstrap(0.4));
  assertOnePerPeriod(r);
  assert.equal(r.leaked, false, 'stop() left a boundary timer running');
});

test('the boundary keeps firing across a setSlotMs() protocol switch', async () => {
  // setSlotMs() restarts the loop, and period indices are slot-relative — an
  // FT8 index is about half the FT4 index for the same instant. A high-water
  // mark carried across the restart would leave every new index below it and
  // wedge the loop, which is why start() clears it.
  let starts = 0;
  const mgr = new FT8PeriodManager({ onPeriodStart: () => { starts++; } }, SLOT);
  mgr.start();
  await sleep(SLOT * 2 + 50);
  const beforeSwitch = starts;
  assert.ok(beforeSwitch > 0, 'no boundary fired before the switch');

  mgr.setSlotMs(SLOT * 2);        // shorter index sequence, as FT4 -> FT8 is
  await sleep(SLOT * 2 * 3 + 50);
  mgr.stop();
  assert.ok(starts > beforeSwitch, 'boundary stopped firing after setSlotMs()');
});
