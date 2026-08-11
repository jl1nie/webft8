// SPDX-License-Identifier: GPL-3.0-or-later
//
// Guards the dependency between mfsk-core's report rendering and qso.js's
// report parsing. The parser matches `[+-]\d{2}`; mfsk-core rendered
// single-digit negatives as "-8" until 0.9.1 (d640bee), and against that
// spelling a station reporting us -1..-9 stalled the QSO outright.
//
//   node --test tests/unit/
//
// If "-8" ever starts reaching FINAL the parser was loosened — decide what the
// log should record before allowing that. If "-08" ever stops reaching FINAL,
// something broke.

import { test } from 'node:test';
import assert from 'node:assert/strict';

const { QsoManager } = await import(
  new URL('../../ft8-web/www/qso.js', import.meta.url).href
);

/**
 * Drive the state machine to the point where it is waiting for a report, then
 * hand it the report in the given spelling.
 */
function trial(reportText) {
  const q = new QsoManager({ myCall: 'JA1ABC', myGrid: 'PM95' });
  q.setMyInfo('JA1ABC', 'PM95');
  q.callStation('JL1NIE');                       // -> CALLING
  q.setRxSnr(-8);
  q.processMessage(`JA1ABC JL1NIE ${reportText}`);
  q.processMessage(`JA1ABC JL1NIE R${reportText}`);
  return { state: q.state, rxReport: q.rxReport };
}

test('the WSJT-X spelling of a single-digit negative completes the QSO', () => {
  assert.equal(trial('-08').state, 'FINAL');
});

test('a two-digit negative completes the QSO', () => {
  assert.equal(trial('-15').state, 'FINAL');
});

test('the pre-0.9.1 spelling "-8" does not reach FINAL', () => {
  // Not an aspiration — this records that the parser is deliberately strict.
  // Loosening it is a decision about what the log records, not a bug fix.
  assert.notEqual(trial('-8').state, 'FINAL');
});
