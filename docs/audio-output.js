// FT8 audio waveform playback via Web Audio API.
// Plays the encoded FT8 signal through the selected audio output device.

export class AudioOutput {
  constructor() {
    this.ctx = null;
    this.sourceNode = null;
    this.gainNode = null;
    this.playing = false;
    this.gain = 1.0;
    this._resolve = null;   // pending play() resolver — settled by _finish()
  }

  /**
   * Play an FT8 waveform through the specified audio output.
   * @param {Float32Array} samples — 12 kHz f32 PCM (from encode_ft8)
   * @param {string} [deviceId] — output device ID (optional)
   * @returns {Promise<boolean>} true if the burst played to the end,
   *   false if stop() cut it short (or a later play() took over)
   */
  async play(samples, deviceId) {
    this.stop();

    const sampleRate = 12000;
    const ctx = new AudioContext({ sampleRate });
    this.ctx = ctx;

    // Android Chrome suspends AudioContext without user gesture — resume it
    if (ctx.state === 'suspended') {
      await ctx.resume();
    }

    // Set output device if supported and specified
    if (deviceId && ctx.setSinkId) {
      try { await ctx.setSinkId(deviceId); } catch (e) {
        console.warn('setSinkId failed:', e);
      }
    }

    // stop() (or another play()) may have landed while we were awaiting the
    // resume/setSinkId above — that context is already closed, so bail out
    // rather than building a graph on it.
    if (this.ctx !== ctx) return false;

    const buffer = ctx.createBuffer(1, samples.length, sampleRate);
    buffer.copyToChannel(samples, 0);

    this.sourceNode = ctx.createBufferSource();
    this.sourceNode.buffer = buffer;
    this.gainNode = ctx.createGain();
    this.gainNode.gain.value = this.gain;
    this.sourceNode.connect(this.gainNode);
    this.gainNode.connect(ctx.destination);

    return new Promise((resolve) => {
      this.playing = true;
      this._resolve = resolve;
      this.sourceNode.onended = () => this._finish(true);
      this.sourceNode.start();
    });
  }

  /**
   * Tear the graph down and settle the pending play() promise exactly once.
   * @param {boolean} completed — did the burst reach its natural end?
   */
  _finish(completed) {
    this.playing = false;
    this.sourceNode = null;
    this.gainNode = null;
    if (this.ctx) {
      try { this.ctx.close(); } catch (_) {}
      this.ctx = null;
    }
    const resolve = this._resolve;
    this._resolve = null;
    if (resolve) resolve(completed);
  }

  /** Set output gain (0.0 - 2.0). */
  setGain(value) {
    this.gain = value;
    if (this.gainNode) this.gainNode.gain.value = value;
  }

  /** Compute peak level of samples (for meter display before playback). */
  static peakLevel(samples) {
    let peak = 0;
    for (let i = 0; i < samples.length; i++) {
      const abs = Math.abs(samples[i]);
      if (abs > peak) peak = abs;
    }
    return peak;
  }

  /**
   * Start a continuous test tone at the given frequency.
   * @param {number} freqHz — tone frequency in Hz
   * @param {string} [deviceId] — output device ID (optional)
   */
  async startTone(freqHz, deviceId) {
    this.stop();
    const ctx = new AudioContext();
    this.ctx = ctx;
    if (ctx.state === 'suspended') await ctx.resume();
    if (deviceId && ctx.setSinkId) {
      try { await ctx.setSinkId(deviceId); } catch (_) {}
    }
    if (this.ctx !== ctx) return;   // stopped while awaiting — see play()
    this.sourceNode = ctx.createOscillator();
    this.sourceNode.type = 'sine';
    this.sourceNode.frequency.value = freqHz;
    this.gainNode = ctx.createGain();
    this.gainNode.gain.value = this.gain;
    this.sourceNode.connect(this.gainNode);
    this.gainNode.connect(ctx.destination);
    this.sourceNode.start();
    this.playing = true;
  }

  /**
   * Stop playback immediately. Any in-flight play() resolves with `false`,
   * so its caller can tell an aborted burst from a completed one instead of
   * awaiting a promise that never settles.
   */
  stop() {
    if (this.sourceNode) {
      // Drop onended first: it would otherwise land _finish(true) after the
      // abort and report the burst as completed.
      this.sourceNode.onended = null;
      try { this.sourceNode.stop(); } catch (e) {}
    }
    this._finish(false);
  }
}
