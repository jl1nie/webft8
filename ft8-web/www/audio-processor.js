// AudioWorklet processor for FT8 audio capture.
// Runs on the audio rendering thread — no ES module imports allowed.
//
// Dual-rate design:
//   • Snapshot/period buffer: kept at the AudioContext native rate (typically
//     48 kHz). The decode pipeline (ft8-core) resamples this offline to 12 kHz
//     via resample_to_12k. Avoiding a live MediaStream resampler is what kills
//     the wavy/sinusoidal spectrum artifact on Atom-class tablets.
//   • Waterfall path: boxcar-decimated in this worklet down to ~6 kHz so the
//     main-thread JS FFT load is independent of the native rate. FT8 only
//     needs 100–3000 Hz of display, and 6 kHz Nyquist covers it exactly.

class FT8AudioProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.outputRate = sampleRate; // AudioWorklet global — native AudioContext rate

    const opts = options?.processorOptions || {};
    this.waterfallTargetRate = opts.waterfallTargetRate || 6000;

    // Snapshot/period buffer at native rate (15 seconds worth)
    this.bufferSize = Math.round(this.outputRate * 15);
    this.buffer = new Float32Array(this.bufferSize);
    this.writePos = 0;
    this.recording = false;

    // Waterfall path: boxcar averager + phase-accumulator decimator.
    // Works for both integer (48k/6k = 8) and non-integer (44.1k/6k ≈ 7.35) ratios.
    this.decimRatio = this.outputRate / this.waterfallTargetRate;
    this.decimPhase = 0;
    this.boxSum = 0;
    this.boxN = 0;
    this.waterfallChunkSize = 1024; // → 1024/6000 ≈ 170 ms per chunk
    this.waterfallAccum = new Float32Array(this.waterfallChunkSize);
    this.waterfallPos = 0;

    // Peak level tracking — based on native rate so cadence is rate-independent
    this.peakLevel = 0;
    this.peakFrameCount = 0;
    this.peakReportInterval = Math.round(this.outputRate / 128 * 0.1); // ~100 ms

    this.port.onmessage = (e) => {
      if (e.data.type === 'start') {
        this.recording = true;
        this.writePos = 0;
        this.waterfallPos = 0;
        this.boxSum = 0;
        this.boxN = 0;
        this.decimPhase = 0;
      } else if (e.data.type === 'stop') {
        this.recording = false;
      } else if (e.data.type === 'snapshot') {
        const snapshot = this.buffer.slice(0, this.writePos);
        this.port.postMessage({
          type: 'snapshot',
          samples: snapshot,
          length: this.writePos,
          sampleRate: this.outputRate,
        });
        this.writePos = 0;
        this.waterfallPos = 0;
        this.boxSum = 0;
        this.boxN = 0;
        this.decimPhase = 0;
      }
    };

    // Report rates to main thread
    this.port.postMessage({
      type: 'info',
      nativeRate: this.outputRate,
      waterfallRate: this.waterfallTargetRate,
      bufferSize: this.bufferSize,
    });
  }

  process(inputs) {
    const input = inputs[0]?.[0];
    if (!input || !this.recording) return true;

    // Track peak level
    for (let i = 0; i < input.length; i++) {
      const abs = Math.abs(input[i]);
      if (abs > this.peakLevel) this.peakLevel = abs;
    }
    this.peakFrameCount += input.length;
    if (this.peakFrameCount >= this.peakReportInterval) {
      this.port.postMessage({ type: 'peak', level: this.peakLevel });
      this.peakLevel = 0;
      this.peakFrameCount = 0;
    }

    const buffer = this.buffer;
    const bufferSize = this.bufferSize;
    const wfAccum = this.waterfallAccum;
    const wfChunk = this.waterfallChunkSize;
    const decimRatio = this.decimRatio;

    for (let i = 0; i < input.length; i++) {
      const sample = input[i];

      // (1) Snapshot/period buffer at native rate
      if (this.writePos < bufferSize) {
        buffer[this.writePos++] = sample;
      }

      // (2) Waterfall path: boxcar accumulate, emit one decimated sample
      //     whenever the phase accumulator crosses decimRatio.
      this.boxSum += sample;
      this.boxN++;
      this.decimPhase += 1;
      if (this.decimPhase >= decimRatio) {
        this.decimPhase -= decimRatio;
        const avg = this.boxSum / this.boxN;
        this.boxSum = 0;
        this.boxN = 0;

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

    // Notify when buffer is full
    if (this.writePos >= bufferSize) {
      this.port.postMessage({ type: 'buffer-full' });
    }

    return true;
  }
}

registerProcessor('ft8-audio-processor', FT8AudioProcessor);
