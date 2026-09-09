// SPDX-License-Identifier: GPL-3.0-or-later
//
// A virtual clock for the timer-driven modules under tests/unit/.
//
// Why: FT8PeriodManager aligns to `Date.now()` and arms real `setTimeout`s, so
// a test of "does the slot loop keep up" is really a test of the host's event
// loop. On a loaded CI box it stalls — one observed run lost 1.4 s in the
// middle, which reads exactly like the skipped-slot bug the test exists to
// catch, and there is no way to tell the two apart from inside the process.
// Virtual time cannot starve, so on this clock a skipped slot is the bug and
// nothing else.
//
// Not for properties that are *about* real timers — a `setTimeout` firing a
// hair early, for one. Those still need the real thing.
//
// The module under test must reach the timer functions through the global
// scope (no `import { setTimeout } from 'node:timers'`), which is the case for
// browser code.

/**
 * Replace the global clock and timer functions with a virtual one.
 * Returns `{ advance, now, uninstall }`; always `uninstall()` in a finally, or
 * the test runner is left running on the fake clock.
 */
export function installFakeClock(startMs = 1_700_000_000_000) {
  const real = {
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
    setInterval: globalThis.setInterval,
    clearInterval: globalThis.clearInterval,
    now: Date.now,
  };

  let now = startMs;
  let seq = 0;
  const timers = new Map();

  globalThis.setTimeout = (fn, ms = 0, ...args) => {
    const id = ++seq;
    timers.set(id, { id, at: now + Math.max(0, ms), fn, args, every: null });
    return id;
  };
  globalThis.setInterval = (fn, ms = 0, ...args) => {
    const id = ++seq;
    const every = Math.max(1, ms);
    timers.set(id, { id, at: now + every, fn, args, every });
    return id;
  };
  globalThis.clearTimeout = (id) => { timers.delete(id); };
  globalThis.clearInterval = (id) => { timers.delete(id); };
  Date.now = () => now;

  // Let every already-queued microtask (and the ones they queue) run. The code
  // under test suspends on fake timers, so a bounded drain is enough to reach
  // the next suspension point rather than a heuristic.
  const drain = async () => { for (let i = 0; i < 200; i++) await Promise.resolve(); };

  /** Earliest timer due at or before `limit`, ties broken by arming order. */
  const nextDue = (limit) => {
    let best = null;
    for (const t of timers.values()) {
      if (t.at > limit) continue;
      if (!best || t.at < best.at || (t.at === best.at && t.id < best.id)) best = t;
    }
    return best;
  };

  /** Run virtual time forward by `ms`, firing timers in order as it goes. */
  const advance = async (ms) => {
    const target = now + ms;
    for (;;) {
      const t = nextDue(target);
      if (!t) break;
      now = t.at;
      if (t.every === null) timers.delete(t.id);
      else t.at = now + t.every;
      try { t.fn(...t.args); } catch (e) { queueMicrotask(() => { throw e; }); }
      await drain();
    }
    now = target;
    await drain();
  };

  const uninstall = () => {
    globalThis.setTimeout = real.setTimeout;
    globalThis.clearTimeout = real.clearTimeout;
    globalThis.setInterval = real.setInterval;
    globalThis.clearInterval = real.clearInterval;
    Date.now = real.now;
    timers.clear();
  };

  return { advance, uninstall, now: () => now, pending: () => timers.size };
}
