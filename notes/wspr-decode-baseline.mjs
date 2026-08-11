// SPDX-License-Identifier: GPL-3.0-or-later
//
// WSPR decode baseline harness — false accepts and recall.
//
// Purpose: give the "OSD only uses the hash on Fano-validated data" change in
// mfsk-core a before/after it can actually be judged against. Tightening an
// acceptance rule always trades false accepts for recall, and recall is the
// side that goes quiet when it regresses, so both are measured here.
//
// Runs against the *built WASM the app itself loads*, not against mfsk-core
// directly, so it measures the path a user is actually on. Defaults to `docs/`
// because that is committed and present in any clone; point it at a fresh
// build with WSPR_PKG_DIR=ft8-web/pkg once wasm-pack has run.
//
//   node notes/wspr-decode-baseline.mjs
//   WSPR_PKG_DIR=ft8-web/pkg node notes/wspr-decode-baseline.mjs
//
// Seeds are fixed. Do not change them — the recorded baseline in
// notes/wspr-false-decode-baseline.md is only comparable at these seeds.

import { readFile } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, resolve } from 'node:path';

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const PKG = resolve(REPO, process.env.WSPR_PKG_DIR || 'docs');

const { default: init, encode_wspr, decode_wspr_wav_f32 } =
  await import(pathToFileURL(resolve(PKG, 'ft8_web.js')).href);
await init({ module_or_path: await readFile(resolve(PKG, 'ft8_web_bg.wasm')) });

const FS = 12_000;
const SLOT = FS * 120;
const REF_BW = 2500;               // WSJT-X SNR reference bandwidth
const CALL = 'JL1NIE', GRID = 'PM95', PWR = 37;
const TRUE_MSG = `${CALL} ${GRID} ${PWR}`;
const TX_HZ = 1500;                // centre of the 4-tone group

const NOISE_SEED_BASE = 0x51000, NOISE_SLOTS = 20;
const SIG_SEED_BASE = 0x62000, SIG_SEEDS = 8;
const LEVELS = [-18, -22, -26, -28, -30];

// LCG + Box-Muller, sigma = 1 per sample — same convention as ft8-bench's
// simulators, so injected SNR means the same thing here as it does there.
function gaussian(seed) {
  const MASK = (1n << 64n) - 1n;
  let s = BigInt(seed) + 1n;
  let spare = null;
  const next = () => { s = (s * 6364136223846793005n + 1442695040888963407n) & MASK; return s; };
  return () => {
    if (spare !== null) { const v = spare; spare = null; return v; }
    const u = Number(next() >> 11n) / 2 ** 53 || 1e-12;
    const v = Number(next() >> 11n) / 2 ** 53;
    const mag = Math.sqrt(-2 * Math.log(u));
    spare = mag * Math.sin(2 * Math.PI * v);
    return mag * Math.cos(2 * Math.PI * v);
  };
}

/** One 120 s slot of noise, optionally with a WSPR signal 1.0 s in. */
function slot(seed, snrDb) {
  const g = gaussian(seed);
  const buf = new Float32Array(SLOT);
  for (let i = 0; i < SLOT; i++) buf[i] = g();
  if (snrDb !== null) {
    // A^2/2 = SNR * 2*sigma^2*B/FS  ->  A = sqrt(4*SNR*B/FS)
    const amp = Math.sqrt(4 * 10 ** (snrDb / 10) * REF_BW / FS);
    const sig = encode_wspr(CALL, GRID, PWR, TX_HZ);
    for (let i = 0; i < sig.length && FS + i < SLOT; i++) buf[FS + i] += sig[i] * amp;
  }
  return buf;
}

const fmt = (r) => r.length
  ? r.map(d => `"${d.message}"@${d.freq_hz.toFixed(1)}`).join('  ')
  : '-';

const t0 = Date.now();
console.log('=== WSPR decode baseline ===');
console.log(`    pkg ${PKG}`);
console.log(`    signal: ${TRUE_MSG} @ ${TX_HZ} Hz, dt +1.0 s\n`);

// ── 1. False accepts on noise alone ─────────────────────────────────────────
// Every decode here is false by construction: there is no signal.
let falseOnNoise = 0;
const noiseExamples = [];
for (let i = 0; i < NOISE_SLOTS; i++) {
  const r = decode_wspr_wav_f32(slot(NOISE_SEED_BASE + i, null), FS);
  falseOnNoise += r.length;
  for (const d of r) noiseExamples.push(`seed ${i}: "${d.message}" @${d.freq_hz.toFixed(1)} Hz`);
}
console.log('  [1] NOISE ONLY — every decode is a false accept');
console.log(`      ${NOISE_SLOTS} slots -> ${falseOnNoise} decodes`);
for (const e of noiseExamples) console.log(`      ${e}`);

// ── 2. Recall, and false accepts alongside a real signal ────────────────────
// The half that regresses silently when acceptance is tightened.
console.log('\n  [2] RECALL vs injected SNR');
console.log('      SNR   true   false');
const recall = [];
for (const snr of LEVELS) {
  let hit = 0, bad = 0;
  for (let i = 0; i < SIG_SEEDS; i++) {
    const r = decode_wspr_wav_f32(slot(SIG_SEED_BASE + i, snr), FS);
    if (r.some(d => d.message === TRUE_MSG)) hit++;
    bad += r.filter(d => d.message !== TRUE_MSG).length;
  }
  recall.push([snr, hit, bad]);
  console.log(`      ${String(snr).padStart(3)}   ${`${hit}/${SIG_SEEDS}`.padStart(4)}   ${String(bad).padStart(5)}`);
}

// ── 3. Same seed, signal on vs off ──────────────────────────────────────────
// Separates noise-driven false accepts from signal-driven ones, and shows
// whether a false decode's *content* is stable. It is not: the same noise
// with a signal 126 Hz away produced an entirely different callsign at the
// same frequency, which is what "no CRC" looks like from the outside.
console.log('\n  [3] SAME SEED, signal ON vs OFF');
for (let i = 0; i < 6; i++) {
  const off = decode_wspr_wav_f32(slot(SIG_SEED_BASE + i, null), FS);
  const on = decode_wspr_wav_f32(slot(SIG_SEED_BASE + i, -26), FS);
  console.log(`      seed ${i}  off: ${fmt(off).padEnd(40)} on(-26): ${fmt(on)}`);
}

console.log(`\n    (${((Date.now() - t0) / 1000).toFixed(0)} s)`);
