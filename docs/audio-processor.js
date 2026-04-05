// AudioWorklet processor for FT8 audio capture.
// Runs on the audio rendering thread — no ES module imports allowed.
// Accumulates samples and posts chunks to the main thread.

class FT8AudioProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.bufferSize = 180000; // 15s at 12kHz
    this.buffer = new Float32Array(this.bufferSize);
    this.writePos = 0;
    this.recording = false;
    this.waterfallChunkSize = 1024; // send waterfall data every ~85ms at 12kHz
    this.waterfallAccum = 0;

    this.port.onmessage = (e) => {
      if (e.data.type === 'start') {
        this.recording = true;
        this.writePos = 0;
        this.waterfallAccum = 0;
      } else if (e.data.type === 'stop') {
        this.recording = false;
      } else if (e.data.type === 'snapshot') {
        // Main thread requests current buffer snapshot for decode
        const snapshot = this.buffer.slice(0, this.writePos);
        this.port.postMessage({ type: 'snapshot', samples: snapshot, length: this.writePos });
        // Reset for next period
        this.writePos = 0;
        this.waterfallAccum = 0;
      }
    };
  }

  process(inputs) {
    const input = inputs[0]?.[0]; // mono channel 0
    if (!input || !this.recording) return true;

    const len = input.length;

    // Append to period buffer
    const remaining = this.bufferSize - this.writePos;
    const toCopy = Math.min(len, remaining);
    if (toCopy > 0) {
      this.buffer.set(input.subarray(0, toCopy), this.writePos);
      this.writePos += toCopy;
    }

    // Forward small chunks for waterfall display
    this.waterfallAccum += len;
    if (this.waterfallAccum >= this.waterfallChunkSize) {
      // Send the latest input block for waterfall rendering
      this.port.postMessage({ type: 'waterfall', samples: new Float32Array(input) });
      this.waterfallAccum = 0;
    }

    // Notify when buffer is full (15 seconds captured)
    if (this.writePos >= this.bufferSize) {
      this.port.postMessage({ type: 'buffer-full' });
      // Don't auto-reset — wait for 'snapshot' command from main thread
      // to avoid race condition with period timer
    }

    return true;
  }
}

registerProcessor('ft8-audio-processor', FT8AudioProcessor);
