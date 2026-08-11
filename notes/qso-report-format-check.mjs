// SPDX-License-Identifier: GPL-3.0-or-later
//
// Guards the dependency between mfsk-core's report rendering and qso.js's
// report parsing. The parser matches `[+-]\d{2}`; mfsk-core rendered
// single-digit negatives as "-8" until 0.9.1 (d640bee), and against that
// spelling a station reporting us -1..-9 stalled the QSO outright.
//
//   node notes/qso-report-format-check.mjs
//
// Expected: "-08" and "-15" reach FINAL; "-8" does not. If "-8" ever starts
// reaching FINAL the parser was loosened — decide what the log should record
// before allowing that. If "-08" ever stops reaching FINAL, something broke.

import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, resolve } from 'node:path';
const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const { QsoManager } = await import(pathToFileURL(resolve(REPO, 'ft8-web/www/qso.js')).href);

// Drive the state machine to the point where it is waiting for a report,
// then hand it the same report in both spellings.
function trial(reportText) {
  const q = new QsoManager({ myCall: 'JA1ABC', myGrid: 'PM95' });
  q.setMyInfo('JA1ABC', 'PM95');
  q.callStation('JL1NIE');                       // -> CALLING
  q.setRxSnr(-8);
  const r1 = q.processMessage(`JA1ABC JL1NIE ${reportText}`);
  const afterCalling = { state: q.state, dxGrid: q.dxGrid, rxReport: q.rxReport, tx: r1 && `${r1.call1} ${r1.call2} ${r1.report}` };
  const r2 = q.processMessage(`JA1ABC JL1NIE R${reportText}`);
  return { afterCalling, second: r2 && `${r2.call1} ${r2.call2} ${r2.report}`, rxReport: q.rxReport, state: q.state };
}

for (const rep of ['-8', '-08', '-15']) {
  const t = trial(rep);
  console.log(`  incoming "${rep}"`);
  console.log(`    after 1st msg: state=${t.afterCalling.state} dxGrid=${JSON.stringify(t.afterCalling.dxGrid)} rxReport=${JSON.stringify(t.afterCalling.rxReport)} tx=${JSON.stringify(t.afterCalling.tx)}`);
  console.log(`    after R-msg  : tx=${JSON.stringify(t.second)} rxReport=${JSON.stringify(t.rxReport)} state=${t.state}`);
}

const ok = (r) => r.state === 'FINAL';
const results = ['-8', '-08', '-15'].map(r => [r, trial(r)]);
const pass = !ok(results[0][1]) && ok(results[1][1]) && ok(results[2][1]);
console.log(pass
  ? '\nPASS - WSJT-X spelling completes the QSO, the old spelling does not'
  : '\nFAIL - see the table above');
process.exit(pass ? 0 : 1);
