// SPDX-License-Identifier: GPL-3.0-or-later
//
// Guards the slot grid against a decode that outlasts its slot (issue #11,
// defect 2'). Run with:
//
//   node --test tests/unit/
//
// The bug: `_scheduleBoundary()` was called only *after* the awaited
// `onPeriodEnd`, so the boundary timer was armed for the next slot only once
// the decode of the previous one had finished. A decode longer than a slot
// therefore armed the timer for a boundary that had already gone by, and
// `_scheduleBoundary()` targeted the one after it. The slot in between was not
// merely un-decoded — it never fired at all, so it lost its `onPeriodStart`
// (the WSPR beacon schedule) and its queued TX with it, and the worklet's
// capture buffer, never asked for a snapshot, kept serving the stale side.
// Measured on a 200 ms slot with decode at 1.3x slot: 5 of 12 slots gone.
//
// Decode still cannot *overlap* — the worker's WASM instance keeps its audio,
// FFT and phase-1 candidate cache between the phase 1 and phase 2 calls — so
// the fix is not concurrency. It is that the timer stops waiting for the
// decode, and the decode is told (`overrun`) to shed when the previous one is
// still running. Everything that must happen every slot happens every slot.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { installFakeClock } from '../helpers/fake-clock.mjs';

const { FT8PeriodManager } = await import(
  new URL('../../ft8-web/www/ft8-period.js', import.meta.url).href
);

const SLOT = 200;
const SLOTS = 12;
const OVERRUN = Math.round(SLOT * 1.3);   // decode outlasts its slot

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * The two run-length tests below count slots over a dozen periods, and on real
 * timers a host stall is indistinguishable from the bug: one observed run lost
 * 1.4 s mid-test and dropped seven slots, which is what a skipping loop looks
 * like. Virtual time cannot stall, so here a lost slot is the code's doing.
 */
async function onFakeClock(body) {
  const clock = installFakeClock();
  try {
    await body(clock);
  } finally {
    clock.uninstall();
  }
}

test('the next boundary is armed before the decode is awaited', async () => {
  // The structural form of the fix, asserted without depending on host timing:
  // by the time the handler is inside `onPeriodEnd`, a boundary timer for the
  // next slot must already be pending. Under the old order there was none —
  // the arming came after this callback returned.
  let armed = null;
  let fireAhead = null;

  const mgr = new FT8PeriodManager({
    onPeriodEnd: async () => {
      if (armed !== null) return;
      armed = mgr.boundaryTimeout !== null;
      fireAhead = mgr._nextFireMs - Date.now();
      await sleep(OVERRUN);
    },
  }, SLOT);

  mgr.start();
  await sleep(SLOT * 2 + OVERRUN + 50);
  mgr.stop();

  assert.equal(armed, true, 'no boundary timer pending during the decode');
  assert.ok(
    fireAhead > 0 && fireAhead <= SLOT,
    `next boundary was ${fireAhead} ms away; expected within one ${SLOT} ms slot`,
  );
});

test('a decode longer than the slot does not skip a slot', async () => {
  await onFakeClock(async ({ advance }) => {
    const starts = [];
    const ends = [];

    const mgr = new FT8PeriodManager({
      onPeriodStart: (i) => { starts.push(i); },
      onPeriodEnd: async (i) => { ends.push(i); await sleep(OVERRUN); },
    }, SLOT);

    mgr.start();
    await advance(SLOT * SLOTS + 50);
    mgr.stop();
    await advance(OVERRUN + SLOT);

    assert.ok(starts.length >= SLOTS - 1, `only ${starts.length} boundaries in ~${SLOTS} slots`);

    // Not one index missing. The unfixed loop loses five or six of twelve.
    let skipped = 0;
    for (let i = 1; i < starts.length; i++) skipped += starts[i] - starts[i - 1] - 1;
    assert.equal(skipped, 0, `${skipped} slots skipped; indices ${starts.join(',')}`);

    // `onPeriodEnd` is offered every slot too — shedding is the handler's call
    // (app.js sheds only the decode, after the capture read), not something
    // the manager decides by not calling.
    assert.equal(ends.length, starts.length, 'onPeriodEnd was not offered every slot');
  });
});

test('overrun marks exactly the slots whose predecessor is still running', async () => {
  // What app.js keys off: shed the decode, keep the snapshot. Asserts both the
  // flag's meaning and that a handler obeying it never runs two decodes at
  // once — which would clobber the decoder's cross-call phase-1 cache.
  await onFakeClock(async ({ advance }) => {
    const seen = [];
    let inDecode = 0;
    let overlapped = false;

    const mgr = new FT8PeriodManager({
      onPeriodEnd: async (i, isEven, { overrun } = {}) => {
        seen.push(overrun);
        if (overrun) return;              // shed, as app.js does
        inDecode++;
        if (inDecode > 1) overlapped = true;
        await sleep(OVERRUN);
        inDecode--;
      },
    }, SLOT);

    mgr.start();
    await advance(SLOT * SLOTS + 50);
    mgr.stop();
    await advance(OVERRUN + SLOT);

    assert.equal(overlapped, false, 'two decodes ran at once despite the overrun flag');
    assert.equal(seen[0], false, 'the first slot of a run cannot be an overrun');
    assert.ok(seen.includes(true), 'a 1.3x-slot decode never reported an overrun');
    // A decode of 1.3 slots blocks the next boundary and is done before the one
    // after, so overruns land on alternate slots and never back to back.
    for (let i = 1; i < seen.length; i++) {
      assert.ok(
        !(seen[i] && seen[i - 1]),
        `two consecutive overruns at ${i - 1}: ${seen.map((b) => (b ? '1' : '0')).join('')}`,
      );
    }
  });
});
