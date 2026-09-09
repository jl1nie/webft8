// Audio device capture for FT8 decoding.
// Handles getUserMedia, AudioContext setup, and resampling to 12kHz.

export class AudioCapture {
  /**
   * @param {Object} callbacks
   * @param {function(Float32Array)} callbacks.onWaterfall - small audio chunks for waterfall
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

    // ── Audio-clock anchor ────────────────────────────────────────────────
    // `frameEpochMs` is the local-clock time (Date.now() ms) at audio frame 0
    // of this AudioContext. With it, any UTC instant maps to a frame index,
    // which is how `snapshotSlot` asks the worklet for a slot by its place on
    // the clock rather than by "whatever has arrived since last time".
    //
    // Each estimate comes from a worklet message carrying the frame counter:
    //   candidate = arrivalTime - frame / rate
    // The message can only ever arrive *late*, never early, so every candidate
    // over-estimates the epoch by its own delivery delay and the true value is
    // the minimum — the same argument NTP's clock filter runs on. A sliding
    // window keeps it tracking the slow drift between the audio device clock
    // and the system clock (a few ppm) instead of latching onto one lucky
    // early sample for the whole session.
    //
    // A backwards system-clock step is followed at once (the candidates drop
    // and so does the minimum); a forwards step takes until the window flushes
    // (~30 s). Neither is a case DT correction handles better.
    this.frameEpochMs = null;
    this._epochSamples = [];
    this._EPOCH_WINDOW = 300;   // ~30 s of ~100 ms peak reports

    // In-flight snapshot requests, keyed by request id. A single pending
    // slot would be enough while the period loop awaited each decode before
    // opening the next boundary, but it no longer does (ft8-period.js), so
    // two handlers can be alive at once and the second must not orphan the
    // first's promise.
    this._snapSeq = 0;
    this._snapPending = new Map();
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

    // Force AudioContext to 12 kHz. Empirically (across Atom tablets, Ryzen 9
    // with high-end DAC at 384 kHz mixer, and a generic 48 kHz mic input),
    // this is the *least bad* configuration: Chrome's polyphase resampler
    // produces a clean 48k → 12k stream, while every other rate combination
    // we tried (native rate, mic rate, in-worklet boxcar at 48 kHz) produced
    // a wavy/sinusoidal spectrum. The 12 kHz path engages Chrome's offline
    // SINC resampler which smooths out source-side clock jitter, whereas a
    // matched rate just hands us whatever the source delivers (jitter and all).
    this.audioCtx = new AudioContext({ sampleRate: 12000 });
    this.actualSampleRate = this.audioCtx.sampleRate;

    const constraints = {
      audio: {
        deviceId: deviceId ? { exact: deviceId } : undefined,
        echoCancellation: false,
        noiseSuppression: false,
        autoGainControl: false,
      }
    };
    this.stream = await navigator.mediaDevices.getUserMedia(constraints);
    const tracks = this.stream.getAudioTracks();
    const trackSettings = tracks[0]?.getSettings?.() || {};
    const micRate = trackSettings.sampleRate || 'unknown';
    console.log(
      `AudioCapture: mic device reports ${micRate} Hz, AudioContext = ${this.actualSampleRate} Hz`
    );

    // Detect device disconnection
    for (const track of tracks) {
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

    // Worklet boxcar-decimates the waterfall path to 6 kHz internally
    // (snapshot path stays at 12 kHz). Halves the main-thread FFT cost
    // while keeping bin width identical to the old 12k/2048 setup.
    this.workletNode = new AudioWorkletNode(this.audioCtx, 'ft8-audio-processor', {
      processorOptions: { waterfallTargetRate: 6000 },
    });

    // Re-apply a previously requested slot buffer size (survives a
    // stop()/start() cycle) so long-period modes keep their larger buffer.
    if (this._bufferSeconds) {
      this.workletNode.port.postMessage({ type: 'setBufferSeconds', seconds: this._bufferSeconds });
    }

    // Handle messages from worklet
    this.workletNode.port.onmessage = (e) => {
      const msg = e.data;
      if (msg.type === 'info') {
        // Snapshot rate (= AudioContext rate) is what runDecode uses;
        // waterfall rate is what the spectrogram FFT runs at.
        this.actualSampleRate = msg.snapshotRate || msg.outputRate;
        this.waterfallRate = msg.waterfallRate || msg.outputRate;
        console.log(
          `Audio: native=${msg.nativeRate} Hz, snapshot=${this.actualSampleRate} Hz, waterfall=${this.waterfallRate} Hz`
        );
        if (this.onSampleRate) this.onSampleRate(this.waterfallRate);
      } else if (msg.type === 'waterfall' && this.callbacks.onWaterfall) {
        this.callbacks.onWaterfall(msg.samples);
      } else if (msg.type === 'peak') {
        if (msg.frame != null) this._noteFrame(msg.frame);
        if (this.onPeak) this.onPeak(msg.level);
      } else if (msg.type === 'snapshot') {
        const done = this._snapPending.get(msg.id);
        if (done) { this._snapPending.delete(msg.id); done(msg); }
      }
    };

    // Insert gain node for input level control
    this.gainNode = this.audioCtx.createGain();
    this.gainNode.gain.value = 1.0;
    source.connect(this.gainNode);
    this.gainNode.connect(this.workletNode);
    // Don't connect to destination (we don't want to play back)

    // Frame numbering belongs to this AudioContext; a previous context's
    // anchor would place every slot in the wrong place.
    this.frameEpochMs = null;
    this._epochSamples = [];

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
    this.frameEpochMs = null;
    this._epochSamples = [];
    // Nothing will answer these now; settle them so no caller is left hanging
    // for the 5 s timeout after an explicit stop.
    for (const [, done] of this._snapPending) {
      done({ samples: new Float32Array(0), sampleRate: this.actualSampleRate,
             missingHead: 0, missingTail: 0, stopped: true });
    }
    this._snapPending.clear();
  }

  /**
   * Fold one (frame, now) pair into the epoch estimate. `frame` is the frame
   * counter at the end of the worklet block that posted the message, so the
   * candidate is biased late by the delivery delay and never early — hence
   * the minimum over the window rather than the mean.
   */
  _noteFrame(frame) {
    const candidate = Date.now() - (frame / this.actualSampleRate) * 1000;
    this._epochSamples.push(candidate);
    if (this._epochSamples.length > this._EPOCH_WINDOW) this._epochSamples.shift();
    let min = this._epochSamples[0];
    for (let i = 1; i < this._epochSamples.length; i++) {
      if (this._epochSamples[i] < min) min = this._epochSamples[i];
    }
    this.frameEpochMs = min;
  }

  /** Spread of the anchor window, in ms — a rough read on delivery jitter. */
  get epochSpreadMs() {
    if (this._epochSamples.length < 2) return null;
    let min = this._epochSamples[0], max = this._epochSamples[0];
    for (const c of this._epochSamples) { if (c < min) min = c; if (c > max) max = c; }
    return max - min;
  }

  /**
   * Ask the worklet for the slot that begins at `startMs` (local-clock ms,
   * i.e. the same scale as `Date.now()`) and runs for `durationMs`.
   *
   * This is the UTC anchor: the window is chosen on the clock and converted to
   * audio frames, so the latency of this very request cannot move it. Ask late
   * and you get the same samples, just nearer the ring's trailing edge. Before
   * the epoch estimate exists — the first ~100 ms of a session, until the
   * first frame-stamped message lands — `startMs` is ignored and the newest
   * `durationMs` of audio is returned, which is what the old fill-and-rewind
   * buffer did on every slot.
   *
   * Resolves to `{ samples, sampleRate, missingHead, missingTail, ... }`.
   * `missingHead`/`missingTail` are counts of zero-padded frames: a head means
   * the request came so late the slot had fallen off the ring, a tail means it
   * came before the audio existed. Both should be 0 in normal running.
   *
   * Automatically resumes the AudioContext if Chrome auto-suspended it
   * (happens after a period of no user interaction). Without this the worklet
   * stops processing and the promise would never resolve; a 5-second timeout
   * covers the case where it fails to respond anyway, so the period loop is
   * never held up (it no longer waits on this at all — see ft8-period.js).
   *
   * @param {number|null} startMs — local-clock ms at the first sample wanted
   * @param {number} durationMs — window length in ms
   */
  snapshotSlot(startMs, durationMs) {
    // Resume if browser auto-suspended the AudioContext.
    if (this.audioCtx?.state === 'suspended') {
      this.audioCtx.resume().catch(() => {});
    }
    const rate = this.actualSampleRate;
    const nFrames = Math.max(1, Math.round((durationMs / 1000) * rate));
    const fromFrame = (startMs != null && this.frameEpochMs != null)
      ? Math.round(((startMs - this.frameEpochMs) / 1000) * rate)
      : null;

    const id = ++this._snapSeq;
    return new Promise((resolve) => {
      const empty = {
        samples: new Float32Array(0), sampleRate: rate,
        missingHead: 0, missingTail: 0, timedOut: true,
      };
      if (!this.workletNode) { resolve(empty); return; }
      const timer = setTimeout(() => {
        this._snapPending.delete(id);
        resolve(empty);
      }, 5000);
      this._snapPending.set(id, (msg) => {
        clearTimeout(timer);
        resolve(msg);
      });
      this.workletNode.port.postMessage({ type: 'snapshot', id, fromFrame, nFrames });
    });
  }

  /**
   * Resize the worklet capture ring to hold `sec` seconds of audio. Callers
   * pass slot + margin: the ring must still hold the whole slot when the
   * request for it arrives, and that request comes after the slot has ended.
   * Long slots (WSPR 120 s, Q65-60, FST4-30..300) need more than the 15 s
   * default or the window falls off the trailing edge. Remembered so it
   * re-applies across stop()/start().
   */
  setBufferSeconds(sec) {
    this._bufferSeconds = sec;
    this.workletNode?.port.postMessage({ type: 'setBufferSeconds', seconds: sec });
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
