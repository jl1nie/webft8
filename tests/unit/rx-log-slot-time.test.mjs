// SPDX-License-Identifier: GPL-3.0-or-later
//
// The RX log records the slot a signal was received in, not the moment the
// decoder happened to finish with it.
//
//   node --test tests/unit/
//
// mfsk-core #313 found this on the embedded WSPR path: the reporting side read
// the clock ~200 s after the window it was reporting on had opened, so every
// spot was filed against the slot after next. WebFT8 had the same shape —
// `addRx()` stamped `new Date()` at write time, and it is called after decode,
// which runs 1-17 s past the slot close depending on how much of phase 2 is
// shed. The chat view was already labelling from the period index, so the
// screen and the CSV/ADIF export disagreed about the same reception.

import { test } from 'node:test';
import assert from 'node:assert/strict';

const { QsoLog } = await import(
  new URL('../../ft8-web/www/qso-log.js', import.meta.url).href
);

// qso-log.js persists through localStorage; give it somewhere to write.
globalThis.localStorage ??= {
  _v: new Map(),
  getItem(k) { return this._v.has(k) ? this._v.get(k) : null; },
  setItem(k, v) { this._v.set(k, String(v)); },
  removeItem(k) { this._v.delete(k); },
};

const SLOT_MS = 15000;

test('addRx files the reception under the slot it was received in', () => {
  const log = new QsoLog();
  const slotStart = new Date('2026-09-09T12:34:15.000Z');

  log.addRx({
    message: 'CQ JL1NIE PM95',
    freq_hz: 1234.5,
    snr_db: -12,
    utc: slotStart.toISOString(),
  });

  const [entry] = log.getRxLog().slice(-1);
  assert.equal(entry.utc, slotStart.toISOString());
});

test('a decode finishing a slot late does not move the logged time', () => {
  const log = new QsoLog();
  const slotStart = Date.UTC(2026, 8, 9, 12, 34, 15);

  // Two receptions from the same slot, logged at different points in a decode
  // that overran into the following slot. Both must carry the slot's own time.
  log.addRx({ message: 'CQ JA1ABC PM95', freq_hz: 700, snr_db: -5,
              utc: new Date(slotStart).toISOString() });
  log.addRx({ message: 'CQ JL1NIE PM95', freq_hz: 1400, snr_db: -20,
              utc: new Date(slotStart).toISOString() });

  const stamps = new Set(log.getRxLog().map(e => e.utc));
  assert.equal(stamps.size, 1, `one slot should give one timestamp, got ${[...stamps]}`);
  assert.equal([...stamps][0], new Date(slotStart).toISOString());

  // And it must not have drifted into the next slot, which is what stamping
  // at write time produced.
  const logged = Date.parse([...stamps][0]);
  assert.ok(logged < slotStart + SLOT_MS, 'timestamp landed in the following slot');
});

test('a caller with no slot of its own still gets a timestamp', () => {
  // The dropped-WAV path has no period index; falling back to now() is right
  // there, and must not throw or record undefined.
  const log = new QsoLog();
  log.addRx({ message: 'CQ JL1NIE PM95', freq_hz: 1000, snr_db: 0 });
  const [entry] = log.getRxLog().slice(-1);
  assert.ok(Number.isFinite(Date.parse(entry.utc)), `bad fallback stamp: ${entry.utc}`);
});
