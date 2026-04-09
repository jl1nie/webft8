// Audio device capture for FT8 decoding.
// Handles getUserMedia, AudioContext setup, and resampling to 12kHz.

export class AudioCapture {
  /**
   * @param {Object} callbacks
   * @param {function(Float32Array)} callbacks.onWaterfall - small audio chunks for waterfall
   * @param {function()} callbacks.onBufferFull - 15-second buffer is full
   */
  constructor(callbacks) {
    this.callbacks = callbacks;
    this.audioCtx = null;
    this.stream = null;
    this.workletNode = null;
    this.gainNode = null;
    this.running = false;
    this.actualSampleRate = 12000;
    this._onDisconnect = null; // callback when device disconnects
    this.onPeak = null; // callback(level: 0-1) for input level meter
    this.onSampleRate = null; // callback(rate) when actual sample rate is determined
  }

  /** Enumerate available audio input devices. */
  async enumerateDevices() {
    // Need a temporary getUserMedia call to get device labels
    try {
      const tmp = await navigator.mediaDevices.getUserMedia({ audio: true });
      tmp.getTracks().forEach(t => t.stop());
    } catch (e) {
      // Permission denied — return empty list
      return [];
    }

    const devices = await navigator.mediaDevices.enumerateDevices();
    return devices
      .filter(d => d.kind === 'audioinput')
      .map(d => ({ id: d.deviceId, label: d.label || `Device ${d.deviceId.slice(0, 8)}` }));
  }

  /**
   * Start capturing audio from the specified device.
   * @param {string} deviceId - audio device ID (from enumerateDevices)
   */
  async start(deviceId) {
    if (this.running) return;

    // Open AudioContext at the device's native rate. Forcing 12 kHz here
    // makes Chrome insert a live MediaStream resampler whose periodic slip
    // correction shows up as wavy/sinusoidal spectrum on weak-clock devices
    // (e.g. Atom tablets). Native rate avoids the resampler entirely; the
    // 48k→12k conversion happens later, offline, inside ft8-core's
    // resample_to_12k (called from the WASM decode entry points).
    this.audioCtx = new AudioContext();
    this.actualSampleRate = this.audioCtx.sampleRate;

    // Get audio stream — disable all processing for clean radio audio
    const constraints = {
      audio: {
        deviceId: deviceId ? { exact: deviceId } : undefined,
        echoCancellation: false,
        noiseSuppression: false,
        autoGainControl: false,
      }
    };

    this.stream = await navigator.mediaDevices.getUserMedia(constraints);

    // Detect device disconnection
    for (const track of this.stream.getTracks()) {
      track.onended = () => {
        if (this.running) {
          this.stop();
          if (this._onDisconnect) this._onDisconnect();
        }
      };
    }

    const source = this.audioCtx.createMediaStreamSource(this.stream);

    // Load AudioWorklet
    const processorUrl = new URL('audio-processor.js', import.meta.url).href;
    await this.audioCtx.audioWorklet.addModule(processorUrl);

    // Worklet runs at the AudioContext's native rate. The waterfall path is
    // boxcar-decimated inside the worklet to 6 kHz (FT8 only needs 100-3000 Hz)
    // to keep the main-thread FFT load constant regardless of native rate.
    this.workletNode = new AudioWorkletNode(this.audioCtx, 'ft8-audio-processor', {
      processorOptions: { waterfallTargetRate: 6000 },
    });

    // Handle messages from worklet
    this.workletNode.port.onmessage = (e) => {
      const msg = e.data;
      if (msg.type === 'info') {
        // Snapshot path is at native rate (msg.nativeRate); waterfall path is
        // decimated to msg.waterfallRate. Consumers (decode) should use native.
        this.actualSampleRate = msg.nativeRate;
        this.waterfallRate = msg.waterfallRate;
        console.log(`Audio: native=${msg.nativeRate} Hz, waterfall=${msg.waterfallRate} Hz`);
        if (this.onSampleRate) this.onSampleRate(msg.nativeRate, msg.waterfallRate);
      } else if (msg.type === 'waterfall' && this.callbacks.onWaterfall) {
        this.callbacks.onWaterfall(msg.samples);
      } else if (msg.type === 'buffer-full' && this.callbacks.onBufferFull) {
        this.callbacks.onBufferFull();
      } else if (msg.type === 'peak' && this.onPeak) {
        this.onPeak(msg.level);
      } else if (msg.type === 'snapshot' && this._snapshotResolve) {
        this._snapshotResolve(msg.samples);
        this._snapshotResolve = null;
      }
    };

    // Insert gain node for input level control
    this.gainNode = this.audioCtx.createGain();
    this.gainNode.gain.value = 1.0;
    source.connect(this.gainNode);
    this.gainNode.connect(this.workletNode);
    // Don't connect to destination (we don't want to play back)

    this.workletNode.port.postMessage({ type: 'start' });
    this.running = true;
  }

  /** Stop capturing. */
  stop() {
    if (!this.running) return;
    this.workletNode?.port.postMessage({ type: 'stop' });
    this.stream?.getTracks().forEach(t => t.stop());
    this.audioCtx?.close();
    this.workletNode = null;
    this.stream = null;
    this.audioCtx = null;
    this.running = false;
  }

  /**
   * Request a snapshot of the current buffer for decoding.
   * Returns a Promise<Float32Array> with the accumulated samples.
   */
  snapshot() {
    return new Promise((resolve) => {
      this._snapshotResolve = resolve;
      this.workletNode?.port.postMessage({ type: 'snapshot' });
    });
  }

  /** Set input gain (0.0 - 2.0). */
  setGain(value) {
    if (this.gainNode) this.gainNode.gain.value = value;
  }

  /** Get the actual sample rate of the AudioContext. */
  getSampleRate() {
    return this.actualSampleRate;
  }
}
