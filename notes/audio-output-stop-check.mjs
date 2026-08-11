// Stubs just enough Web Audio to exercise AudioOutput's play/stop lifecycle.
//
//   node notes/audio-output-stop-check.mjs
//
// The bug under test (fixed in v0.9.1): stop() left play()'s promise
// unsettled, so the awaiting transmit() never resumed — PTT was never
// dropped and txActive stayed true forever. Against the pre-fix file this
// script fails "stop() settles play()" and then throws
// `TypeError: Cannot read properties of null (reading 'close')` from the
// late onended, which is the signature of the bug.

let closedCount = 0;

class FakeParam { constructor() { this.value = 0; } }
class FakeNode {
  constructor(ctx) { this.ctx = ctx; this.onended = null; this.started = false; this.stopped = false; }
  connect() {}
  start() { this.started = true; }
  stop() { this.stopped = true; }
  /** Simulate the buffer reaching its natural end. */
  fireEnded() { if (this.onended) this.onended(); }
}
class FakeBufferSource extends FakeNode { constructor(ctx) { super(ctx); this.buffer = null; } }
class FakeOscillator extends FakeNode {
  constructor(ctx) { super(ctx); this.type = ''; this.frequency = new FakeParam(); }
}
class FakeGain extends FakeNode { constructor(ctx) { super(ctx); this.gain = new FakeParam(); } }

globalThis.AudioContext = class {
  constructor() { this.state = 'running'; this.destination = {}; this.closed = false; this.lastSource = null; }
  async resume() { this.state = 'running'; }
  createBuffer() { return { copyToChannel() {} }; }
  createBufferSource() { this.lastSource = new FakeBufferSource(this); return this.lastSource; }
  createOscillator() { this.lastSource = new FakeOscillator(this); return this.lastSource; }
  createGain() { return new FakeGain(this); }
  close() { this.closed = true; closedCount++; }
};

const { AudioOutput } = await import(
  new URL('../ft8-web/www/audio-output.js', import.meta.url).href
);

const samples = new Float32Array(12000 * 13);
let failures = 0;
const check = (name, cond) => {
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}`);
  if (!cond) failures++;
};

// ── 1. natural completion resolves true ───────────────────────────────────
{
  const out = new AudioOutput();
  const p = out.play(samples);
  await null; await null;                       // let play() get past its awaits
  const ctx = out.ctx;
  ctx.lastSource.fireEnded();
  const completed = await p;
  check('natural end resolves true', completed === true);
  check('natural end closes the context', ctx.closed === true);
  check('natural end clears playing', out.playing === false);
}

// ── 2. stop() mid-burst resolves false (the reported bug) ─────────────────
{
  const out = new AudioOutput();
  const p = out.play(samples);
  await null; await null;
  const ctx = out.ctx;
  const src = ctx.lastSource;
  const settled = await Promise.race([p, Promise.resolve('PENDING')]);
  check('play() is pending while the burst runs', settled === 'PENDING');

  out.stop();
  const completed = await Promise.race([
    p,
    new Promise((r) => setTimeout(() => r('HUNG'), 50)),
  ]);
  check('stop() settles play() (no hang)', completed !== 'HUNG');
  check('stop() resolves false', completed === false);
  check('stop() stopped the source node', src.stopped === true);
  check('stop() closed the context', ctx.closed === true);
  check('stop() clears playing', out.playing === false);

  // A late onended must not re-settle as "completed".
  src.fireEnded();
  check('late onended is disarmed', src.onended === null);
}

// ── 3. stop() while play() is still awaiting resume/setSinkId ─────────────
{
  const out = new AudioOutput();
  const p = out.play(samples);
  out.stop();                                    // abort before the graph exists
  const completed = await Promise.race([
    p,
    new Promise((r) => setTimeout(() => r('HUNG'), 50)),
  ]);
  check('abort during setup settles play()', completed !== 'HUNG');
  check('abort during setup resolves false', completed === false);
}

// ── 4. a second play() supersedes the first ───────────────────────────────
{
  const out = new AudioOutput();
  const p1 = out.play(samples);
  const p2 = out.play(samples);
  await null; await null;
  const first = await Promise.race([p1, new Promise((r) => setTimeout(() => r('HUNG'), 50))]);
  check('superseded play() resolves false', first === false);
  out.ctx.lastSource.fireEnded();
  check('the newer play() still completes', (await p2) === true);
}

// ── 5. test tone: stop() tears it down ────────────────────────────────────
{
  const out = new AudioOutput();
  await out.startTone(1500);
  check('startTone marks playing', out.playing === true);
  const ctx = out.ctx;
  out.stop();
  check('stop() clears tone playing flag', out.playing === false);
  check('stop() closed the tone context', ctx.closed === true);
}

// ── 6. stop() is idempotent ───────────────────────────────────────────────
{
  const out = new AudioOutput();
  const before = closedCount;
  out.stop(); out.stop();
  check('stop() on an idle output is a no-op', closedCount === before);
}

console.log(failures === 0 ? '\nAll checks passed.' : `\n${failures} check(s) FAILED.`);
process.exit(failures === 0 ? 0 : 1);
