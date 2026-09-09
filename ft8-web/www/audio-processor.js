// AudioWorklet processor for FT8 audio capture.
// Runs on the audio rendering thread — no ES module imports allowed.
//
// AudioContext is forced to 12 kHz on the JS side, so the worklet sees
// 12 kHz samples directly. The two output paths are:
//
//   • Capture ring — kept at 12 kHz (no decimation, no boxcar). Handed to
//     the WASM decoder verbatim. Touching this path is what broke earlier
//     iterations; it MUST stay a plain copy of the input block.
//
//   • Waterfall chunks — boxcar-decimated 12 kHz → 6 kHz (factor 2 by
//     default) so the main-thread JS FFT can run at fftSize=1024 with
//     the same 5.86 Hz/bin resolution as the old 12k/2048 setup, at
//     about half the CPU cost. Visually identical for FT8.
//
// The 6 kHz target is configurable via processorOptions.waterfallTargetRate.
// Falls back to passthrough if the worklet rate is at or below the target.
//
// ── Why the capture path is a ring addressed by absolute frame ──────────────
//
// It used to be fill-and-rewind: samples accumulated from index 0, and the
// main thread's `snapshot` message both read the buffer and reset the write
// pointer. The window handed to the decoder was therefore "everything since
// the last time a snapshot message happened to be serviced" — a span with no
// UTC phase of its own (webft8 issue #11, defect 1, the shape mfsk-core #313
// found on embedded). Its start carried the main thread's latency at the
// previous boundary: up to one 128-frame render quantum of message-arrival
// quantisation (10.67 ms at 12 kHz), plus whatever jank — waterfall draw, GC,
// UI — sat between the boundary firing and `port.postMessage` running. The
// constant part of that is absorbed by DT auto-correction; the jitter is not,
// and at 10 ms it is the size of the whole time-sync step this project aims
// for (CLAUDE.md §3.2). It also dropped the *newest* samples once full, and
// stayed full for as long as no snapshot was requested.
//
// Now every sample is stored at its own absolute frame index — `currentFrame`,
// the render-thread frame counter, which is exactly `AudioContext.currentTime
// * sampleRate` — and the main thread asks for a frame *range*. The window is
// then a property of the audio clock, and the request's own latency cannot
// move it: a late request returns the same samples, just closer to the ring's
// trailing edge. Reads are non-destructive, so a slot the app sheds (or asks
// for twice) costs nothing.
//
// What this does NOT remove is the fixed input latency between the ADC and
// the frame reaching this worklet (device buffer + Chrome's 48k→12k resampler
// delay). That is a constant, so DT auto-correction absorbs it, which is what
// it could never do with a jittering window.

class FT8AudioProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.outputRate = sampleRate; // AudioWorklet global — should be 12000

    const opts = options?.processorOptions || {};
    const waterfallTargetRate = opts.waterfallTargetRate || 6000;

    // Capture ring — 15 s at outputRate by default; `setBufferSeconds` sizes
    // it to slot + margin, the margin being how late the main thread may ask
    // for a slot that has already ended.
    this._allocRing(Math.round(this.outputRate * 15));
    this.recording = false;

    // Waterfall path: boxcar averager + phase-accumulator decimator.
    // At outputRate=12k and waterfallTargetRate=6k, decimRatio is exactly 2.
    this.waterfallRate = Math.min(this.outputRate, waterfallTargetRate);
    this.wfDecimRatio = this.outputRate / this.waterfallRate;
    this.wfDecimPhase = 0;
    this.wfBoxSum = 0;
    this.wfBoxN = 0;

    // 512 samples at 6 kHz → 85 ms per chunk → ~12 fps render cadence,
    // matching the original 12k/1024 cadence.
    this.waterfallChunkSize = 512;
    this.waterfallAccum = new Float32Array(this.waterfallChunkSize);
    this.waterfallPos = 0;

    // Peak level tracking. The report doubles as the main thread's clock
    // anchor sample: it carries the frame counter, and the main thread turns
    // the pair (frame, arrival time) into the UTC time of frame 0.
    this.peakLevel = 0;
    this.peakFrameCount = 0;
    // `peakFrameCount` counts frames, so the interval is frames too. It was
    // `outputRate / 128 * 0.1` — the count of *quanta* in 100 ms, 9 of them —
    // which one 128-frame block already exceeds, so the report went out every
    // quantum: ~94 messages a second instead of 10, and a "peak" measured over
    // 10.7 ms rather than the 100 ms the meter is scaled for. The anchor
    // sample rides on this message, and AudioCapture's window is sized in
    // these reports, so the cadence has to be the one written down.
    this.peakReportInterval = Math.round(this.outputRate * 0.1); // 100 ms of frames

    this.port.onmessage = (e) => {
      if (e.data.type === 'start') {
        this.recording = true;
        this._resetState();
      } else if (e.data.type === 'stop') {
        this.recording = false;
      } else if (e.data.type === 'setBufferSeconds') {
        // Resize the ring to hold a full slot plus the margin. Longer
        // protocols (WSPR 120 s, Q65-60, FST4-30..300) need more than the
        // 15 s default or the requested window falls off the trailing edge.
        // Reallocate only when the size actually changes; the in-flight
        // capture is dropped (resize happens on mode change, not mid-slot).
        const secs = Math.max(1, e.data.seconds || 15);
        const n = Math.round(this.outputRate * secs);
        if (n !== this.ringSize) this._allocRing(n);
      } else if (e.data.type === 'snapshot') {
        this._serveSnapshot(e.data);
      }
    };

    // Report rates to main thread
    this.port.postMessage({
      type: 'info',
      nativeRate: this.outputRate,
      outputRate: this.outputRate,    // legacy alias = snapshot rate
      snapshotRate: this.outputRate,
      waterfallRate: this.waterfallRate,
      bufferSize: this.ringSize,
    });
  }

  _allocRing(size) {
    this.ringSize = size;
    this.ring = new Float32Array(size);
    // Absolute frame index of the next sample to be written, and of the
    // oldest one still standing. -1 until the first block arrives, since
    // `currentFrame` is already well past 0 by the time capture starts.
    this.writeFrame = -1;
    this.firstFrame = -1;
  }

  _resetState() {
    this.writeFrame = -1;
    this.firstFrame = -1;
    this.waterfallPos = 0;
    this.wfBoxSum = 0;
    this.wfBoxN = 0;
    this.wfDecimPhase = 0;
  }

  /** Oldest frame the ring can still answer for. */
  _oldestFrame() {
    return Math.max(this.firstFrame, this.writeFrame - this.ringSize);
  }

  /**
   * Copy `block` into the ring at `this.writeFrame`, wrapping once at most.
   * Two `set()` calls rather than a per-sample modulo — the same plain copy
   * the fill-and-rewind version did, and cheaper.
   */
  _write(block) {
    const pos = this.writeFrame % this.ringSize;
    const head = Math.min(block.length, this.ringSize - pos);
    if (head === block.length) {
      this.ring.set(block, pos);
    } else {
      this.ring.set(block.subarray(0, head), pos);
      this.ring.set(block.subarray(head), 0);
    }
  }

  /**
   * The render thread advanced without us writing (capture toggled off and on
   * again, or a glitch). Zero the frames we never saw so a read across the
   * gap returns silence rather than audio from the ring's previous lap under
   * a frame number it does not belong to. Forward gaps only; the caller
   * handles a counter that does not advance.
   */
  _fillGap(toFrame) {
    const gap = toFrame - this.writeFrame;
    if (gap <= 0) return;
    if (gap >= this.ringSize) {
      this.ring.fill(0);
      this.firstFrame = toFrame;
      this.writeFrame = toFrame;
      return;
    }
    const pos = this.writeFrame % this.ringSize;
    const head = Math.min(gap, this.ringSize - pos);
    this.ring.fill(0, pos, pos + head);
    if (head < gap) this.ring.fill(0, 0, gap - head);
    this.writeFrame = toFrame;
  }

  /**
   * Answer a frame-range request. `fromFrame == null` means "the newest
   * `nFrames`" — the fallback for a main thread that has no clock anchor yet.
   * Always returns exactly `nFrames` samples, zero-padded where the ring
   * cannot answer, and says by how much in `missingHead`/`missingTail` so the
   * caller can tell a genuinely silent slot from one it asked for too late.
   */
  _serveSnapshot(req) {
    const n = Math.max(1, Math.round(req.nFrames) || this.ringSize);
    const from = req.fromFrame == null
      ? Math.max(this._oldestFrame(), this.writeFrame - n)
      : Math.round(req.fromFrame);

    const out = new Float32Array(n);
    const lo = Math.max(from, this._oldestFrame());
    const hi = Math.min(from + n, this.writeFrame);
    if (hi > lo && this.writeFrame >= 0) {
      const count = hi - lo;
      const pos = lo % this.ringSize;
      const head = Math.min(count, this.ringSize - pos);
      out.set(this.ring.subarray(pos, pos + head), lo - from);
      if (head < count) out.set(this.ring.subarray(0, count - head), lo - from + head);
    }

    this.port.postMessage({
      type: 'snapshot',
      id: req.id,
      samples: out,
      length: n,
      sampleRate: this.outputRate,
      fromFrame: from,
      writeFrame: this.writeFrame,
      missingHead: Math.max(0, Math.min(n, lo - from)),
      missingTail: Math.max(0, Math.min(n, (from + n) - Math.max(hi, lo))),
    }, [out.buffer]);
  }

  process(inputs) {
    const input = inputs[0]?.[0];
    if (!input || !this.recording) return true;

    // Track peak level, and hand the main thread a (frame, now) pair to
    // anchor its UTC↔frame mapping with. `currentFrame` is the frame index of
    // this quantum's first sample, so the block ends at currentFrame + length.
    for (let i = 0; i < input.length; i++) {
      const abs = Math.abs(input[i]);
      if (abs > this.peakLevel) this.peakLevel = abs;
    }
    this.peakFrameCount += input.length;
    if (this.peakFrameCount >= this.peakReportInterval) {
      this.port.postMessage({
        type: 'peak',
        level: this.peakLevel,
        frame: currentFrame + input.length,
      });
      this.peakLevel = 0;
      this.peakFrameCount = 0;
    }

    // (1) Capture ring — plain block copy at this quantum's absolute frame.
    //     Goes straight to the WASM decoder.
    //
    //     The write position is taken from `currentFrame` every block rather
    //     than advanced by `input.length`, so the ring's frame numbering is
    //     the render thread's and cannot drift from it. A forward jump is a
    //     gap we never captured (zero-filled); a counter that has not moved
    //     is not something this can happen to in a conformant host, and
    //     rebasing on it is at worst a re-write of frames we already hold.
    const start = currentFrame;
    if (this.writeFrame < 0) {
      this.firstFrame = start;
    } else if (start > this.writeFrame) {
      this._fillGap(start);
    }
    this.writeFrame = start;
    this._write(input);
    this.writeFrame = start + input.length;
    if (this.writeFrame - this.firstFrame > this.ringSize) {
      this.firstFrame = this.writeFrame - this.ringSize;
    }

    // (2) Waterfall path — boxcar accumulate, emit one decimated sample
    //     whenever the phase accumulator crosses the ratio. The capture
    //     path is independent of this; only the visualization is
    //     downsampled.
    const wfAccum = this.waterfallAccum;
    const wfChunk = this.waterfallChunkSize;
    const wfDecimRatio = this.wfDecimRatio;
    for (let i = 0; i < input.length; i++) {
      this.wfBoxSum += input[i];
      this.wfBoxN++;
      this.wfDecimPhase += 1;
      if (this.wfDecimPhase >= wfDecimRatio) {
        this.wfDecimPhase -= wfDecimRatio;
        const avg = this.wfBoxSum / this.wfBoxN;
        this.wfBoxSum = 0;
        this.wfBoxN = 0;

        if (this.waterfallPos < wfChunk) {
          wfAccum[this.waterfallPos++] = avg;
        }
        if (this.waterfallPos >= wfChunk) {
          this.port.postMessage({
            type: 'waterfall',
            samples: new Float32Array(wfAccum),
          });
          this.waterfallPos = 0;
        }
      }
    }

    return true;
  }
}

registerProcessor('ft8-audio-processor', FT8AudioProcessor);
