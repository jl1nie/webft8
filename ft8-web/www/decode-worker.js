// Web Worker for FT8 decode.
//
// Runs the WASM decoder off the main thread so the waterfall keeps scrolling
// (and the UI stays responsive) while a 200-400 ms decode call is in flight.
// The worker has its own WASM instance — duplicates ~400 KB of memory but
// avoids the SharedArrayBuffer / COOP / COEP requirements GitHub Pages
// doesn't set.
//
// Message protocol (main → worker):
//   { id, fn, args }    — fn is one of FN_MAP keys; args is positional
//                         (for STREAMING_FNS, args excludes the on_result
//                         callback — the worker supplies that itself)
//
// Replies (worker → main):
//   { type: 'ready' }                    — once after init() completes
//   { id, type: 'partial', result }      — STREAMING_FNS only, one per
//                                          accepted candidate (mfsk-core 0.9
//                                          `.on_result()`), zero or more,
//                                          arrives before the final reply.
//                                          De-duped by exact message text
//                                          within one call — see the
//                                          `seen` Set below: the "default"
//                                          (non-SIC) wide-band strategies
//                                          (FT8 decode_phase1_streaming,
//                                          FST4 decode_fst4_wav_streaming)
//                                          document that on_result can fire
//                                          more than once for the same
//                                          eventual message (redundant
//                                          near-duplicate sync candidates
//                                          converging on one signal, culled
//                                          by cross-candidate dedup *after*
//                                          on_result already fired — see
//                                          mfsk-core docs/reference/
//                                          STREAMING.md §3b). The final
//                                          `results` Vec is already correct
//                                          either way; this only stops the
//                                          same row from appearing twice in
//                                          the UI before that Vec arrives.
//   { id, ok: true, results }            — decoded messages as plain objects
//   { id, ok: false, error }             — decode threw

import init, {
  // FT8
  decode_wav, decode_wav_subtract, decode_sniper,
  decode_wav_f32, decode_wav_subtract_f32, decode_sniper_f32,
  decode_phase1, decode_phase1_f32,
  decode_phase2, decode_phase2_f32,
  decode_phase1_streaming, decode_phase1_streaming_f32,
  decode_phase2_streaming, decode_phase2_streaming_f32,
  // FT4
  decode_ft4_wav, decode_ft4_wav_f32,
  decode_ft4_wav_subtract, decode_ft4_wav_subtract_f32,
  decode_ft4_wav_subtract_streaming, decode_ft4_wav_subtract_streaming_f32,
  decode_ft4_sniper, decode_ft4_sniper_f32,
  // FST4 — five T/R sub-modes (sub-mode + profile passed as u8)
  decode_fst4_wav, decode_fst4_wav_f32,
  decode_fst4_wav_streaming, decode_fst4_wav_streaming_f32,
  // WSPR
  decode_wspr_wav, decode_wspr_wav_f32,
  decode_wspr_wav_streaming, decode_wspr_wav_streaming_f32,
  // Q65 — basic BP scan + fast-fading metric. (sub-mode passed as u8)
  decode_q65_wav, decode_q65_wav_f32,
  decode_q65_wav_fading, decode_q65_wav_fading_f32,
  decode_q65_wav_streaming, decode_q65_wav_streaming_f32,
  decode_q65_wav_fading_streaming, decode_q65_wav_fading_streaming_f32,
  // Cold-start DT bootstrap (mfsk-core 0.6.6 bootstrap_dt_median)
  bootstrap_dt, bootstrap_dt_f32,
} from '../pkg/ft8_web.js';

const FN_MAP = {
  decode_wav, decode_wav_subtract, decode_sniper,
  decode_wav_f32, decode_wav_subtract_f32, decode_sniper_f32,
  decode_phase1, decode_phase1_f32,
  decode_phase2, decode_phase2_f32,
  decode_phase1_streaming, decode_phase1_streaming_f32,
  decode_phase2_streaming, decode_phase2_streaming_f32,
  decode_ft4_wav, decode_ft4_wav_f32,
  decode_ft4_wav_subtract, decode_ft4_wav_subtract_f32,
  decode_ft4_wav_subtract_streaming, decode_ft4_wav_subtract_streaming_f32,
  decode_ft4_sniper, decode_ft4_sniper_f32,
  decode_fst4_wav, decode_fst4_wav_f32,
  decode_fst4_wav_streaming, decode_fst4_wav_streaming_f32,
  decode_wspr_wav, decode_wspr_wav_f32,
  decode_wspr_wav_streaming, decode_wspr_wav_streaming_f32,
  decode_q65_wav, decode_q65_wav_f32,
  decode_q65_wav_fading, decode_q65_wav_fading_f32,
  decode_q65_wav_streaming, decode_q65_wav_streaming_f32,
  decode_q65_wav_fading_streaming, decode_q65_wav_fading_streaming_f32,
  bootstrap_dt, bootstrap_dt_f32,
};

// FN_MAP keys whose WASM fn takes a trailing on_result(msg) callback (mfsk-core
// 0.9 `.on_result()`, wired in ft8-web/src/lib.rs). The worker builds and
// appends this callback itself — callers just omit it from `args`.
const STREAMING_FNS = new Set([
  'decode_phase1_streaming', 'decode_phase1_streaming_f32',
  'decode_phase2_streaming', 'decode_phase2_streaming_f32',
  'decode_ft4_wav_subtract_streaming', 'decode_ft4_wav_subtract_streaming_f32',
  'decode_fst4_wav_streaming', 'decode_fst4_wav_streaming_f32',
  'decode_wspr_wav_streaming', 'decode_wspr_wav_streaming_f32',
  'decode_q65_wav_streaming', 'decode_q65_wav_streaming_f32',
  'decode_q65_wav_fading_streaming', 'decode_q65_wav_fading_streaming_f32',
]);

const initPromise = init().then(() => {
  self.postMessage({ type: 'ready' });
});

// Convert one WASM-side DecodedMessage instance to a plain JS object and
// free its WASM-side memory. Shared by the batch (toPlain) and per-candidate
// streaming (onResult callback) paths.
function toPlainOne(r) {
  const plain = {
    message: r.message,
    freq_hz: r.freq_hz,
    dt_sec: r.dt_sec,
    snr_db: r.snr_db,
    hard_errors: r.hard_errors,
    pass: r.pass,
  };
  r.free();
  return plain;
}

function toPlain(results) {
  const plain = new Array(results.length);
  for (let i = 0; i < results.length; i++) plain[i] = toPlainOne(results[i]);
  return plain;
}

self.onmessage = async (e) => {
  await initPromise;
  const { id, fn, args } = e.data;
  try {
    const f = FN_MAP[fn];
    if (!f) throw new Error(`unknown decode fn: ${fn}`);
    // Streaming calls get an extra on_result callback appended after `args`.
    // It runs synchronously inside the still-in-flight WASM call, but
    // postMessage queues immediately for the main thread regardless — the
    // worker doesn't need to return to its event loop for delivery.
    // `seen`: de-dupe by exact message text within this one call — see the
    // STREAMING_FNS comment up top for why a "default" wide-band strategy
    // can legitimately fire on_result more than once for the same message.
    const seen = new Set();
    const callArgs = STREAMING_FNS.has(fn)
      ? [...args, (msg) => {
          const plain = toPlainOne(msg);
          if (seen.has(plain.message)) return;
          seen.add(plain.message);
          self.postMessage({ id, type: 'partial', result: plain });
        }]
      : args;
    const results = f(...callArgs);
    // Non-decode helpers (e.g. bootstrap_dt) return scalars, not Vec<DecodedMessage>.
    const payload = (results && typeof results.length === 'number') ? toPlain(results) : results;
    self.postMessage({ id, ok: true, results: payload });
  } catch (err) {
    self.postMessage({ id, ok: false, error: String(err?.message || err) });
  }
};
