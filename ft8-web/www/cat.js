// CAT (Computer Aided Transceiver) control via Web Serial API.
// Supports: Yaesu CAT (FTDX10 default), Icom CI-V.

export class CatController {
  constructor() {
    this.port = null;
    this.writer = null;
    this.connected = false;
    this.protocol = 'yaesu';
    this.civAddress = 0x94;
    this.pttOn = false; // track PTT state
    this.onDisconnect = null; // callback
  }

  static isSupported() {
    return 'serial' in navigator;
  }

  async requestPort() {
    if (!CatController.isSupported()) throw new Error('Web Serial API not supported');
    this.port = await navigator.serial.requestPort();
    return this.port;
  }

  async connect(opts = {}) {
    if (!this.port) throw new Error('No port selected');
    this.protocol = opts.protocol || 'yaesu';
    const defaultBaud = this.protocol === 'yaesu' ? 38400 : 19200;
    await this.port.open({ baudRate: opts.baudRate || defaultBaud });
    if (opts.civAddress !== undefined) this.civAddress = opts.civAddress;
    this.writer = this.port.writable.getWriter();
    this.connected = true;
    this.pttOn = false;

    // Detect serial port disconnect
    if (this.port.readable) {
      this.port.readable.pipeTo(new WritableStream()).catch(() => {
        this._handleDisconnect();
      });
    }
  }

  async disconnect() {
    await this.safePttOff();
    if (this.writer) { this.writer.releaseLock(); this.writer = null; }
    try { if (this.port?.readable) await this.port.close(); } catch (_) {}
    this.connected = false;
    this.pttOn = false;
  }

  /**
   * Set PTT state.
   * @param {boolean} on — true = transmit, false = receive
   */
  async ptt(on) {
    if (!this.connected) return;
    try {
      if (this.protocol === 'yaesu') {
        await this._yaesuSend(on ? 0x08 : 0x88);
      } else {
        await this._civSend(0x1C, 0x00, on ? [0x01] : [0x00]);
      }
      this.pttOn = on;
    } catch (e) {
      // Write failed — likely disconnected
      this._handleDisconnect();
      throw e;
    }
  }

  /** Force PTT OFF, never throws. Call this in error handlers. */
  async safePttOff() {
    if (!this.connected || !this.pttOn) return;
    try {
      await this.ptt(false);
    } catch (_) {
      // Best effort — port may already be dead
      this.pttOn = false;
    }
  }

  async setFreq(freqHz) {
    if (!this.connected) return;
    try {
      if (this.protocol === 'yaesu') {
        await this._yaesuSetFreq(freqHz);
      } else {
        await this._civSetFreq(freqHz);
      }
    } catch (e) {
      this._handleDisconnect();
      throw e;
    }
  }

  // ── Internal ──────────────────────────────────────────────────────────

  _handleDisconnect() {
    this.connected = false;
    this.pttOn = false;
    if (this.onDisconnect) this.onDisconnect();
  }

  async _yaesuSend(cmd) {
    const frame = new Uint8Array([0x00, 0x00, 0x00, 0x00, cmd]);
    await this.writer.write(frame);
  }

  async _yaesuSetFreq(freqHz) {
    let f = Math.round(freqHz / 10);
    const bcd = new Uint8Array(5);
    for (let i = 3; i >= 0; i--) {
      const lo = f % 10; f = Math.floor(f / 10);
      const hi = f % 10; f = Math.floor(f / 10);
      bcd[i] = (hi << 4) | lo;
    }
    bcd[4] = 0x01;
    await this.writer.write(bcd);
  }

  async _civSend(cmd, subCmd, data = []) {
    const frame = [0xFE, 0xFE, this.civAddress, 0xE0, cmd];
    if (subCmd !== null && subCmd !== undefined) frame.push(subCmd);
    frame.push(...data);
    frame.push(0xFD);
    await this.writer.write(new Uint8Array(frame));
  }

  async _civSetFreq(freqHz) {
    const bcd = [];
    let f = Math.round(freqHz);
    for (let i = 0; i < 5; i++) {
      const lo = f % 10; f = Math.floor(f / 10);
      const hi = f % 10; f = Math.floor(f / 10);
      bcd.push((hi << 4) | lo);
    }
    await this._civSend(0x05, null, bcd);
  }
}
