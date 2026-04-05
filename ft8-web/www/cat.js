// CAT (Computer Aided Transceiver) control via Web Serial API.
// Supports: Yaesu CAT (FTDX10 default), Icom CI-V.

export class CatController {
  constructor() {
    this.port = null;
    this.writer = null;
    this.connected = false;
    this.protocol = 'yaesu'; // 'yaesu' or 'civ'
    this.civAddress = 0x94;  // Icom CI-V address (IC-7300 default)
  }

  /** Check if Web Serial API is available. */
  static isSupported() {
    return 'serial' in navigator;
  }

  /** Request a serial port (requires user gesture). */
  async requestPort() {
    if (!CatController.isSupported()) {
      throw new Error('Web Serial API not supported');
    }
    this.port = await navigator.serial.requestPort();
    return this.port;
  }

  /**
   * Connect to the serial port.
   * @param {Object} opts
   * @param {number} opts.baudRate — baud rate (default 38400 for Yaesu, 19200 for Icom)
   * @param {string} [opts.protocol] — 'yaesu' (default) or 'civ'
   * @param {number} [opts.civAddress] — CI-V address (Icom only)
   */
  async connect(opts = {}) {
    if (!this.port) throw new Error('No port selected');
    this.protocol = opts.protocol || 'yaesu';
    const defaultBaud = this.protocol === 'yaesu' ? 38400 : 19200;
    const baudRate = opts.baudRate || defaultBaud;
    if (opts.civAddress !== undefined) this.civAddress = opts.civAddress;

    await this.port.open({ baudRate });
    this.writer = this.port.writable.getWriter();
    this.connected = true;
  }

  /** Disconnect. */
  async disconnect() {
    if (this.writer) { this.writer.releaseLock(); this.writer = null; }
    if (this.port) { await this.port.close(); }
    this.connected = false;
  }

  /**
   * Set PTT state.
   * @param {boolean} on — true = transmit, false = receive
   */
  async ptt(on) {
    if (!this.connected) throw new Error('Not connected');

    if (this.protocol === 'yaesu') {
      // Yaesu CAT: TX (on) = 0x08, RX (off) = 0x88
      await this._yaesuSend(on ? 0x08 : 0x88);
    } else {
      // Icom CI-V: PTT sub-command
      await this._civSend(0x1C, 0x00, on ? [0x01] : [0x00]);
    }
  }

  /**
   * Set VFO frequency.
   * @param {number} freqHz — frequency in Hz
   */
  async setFreq(freqHz) {
    if (!this.connected) throw new Error('Not connected');

    if (this.protocol === 'yaesu') {
      await this._yaesuSetFreq(freqHz);
    } else {
      await this._civSetFreq(freqHz);
    }
  }

  // ── Yaesu CAT protocol ────────────────────────────────────────────────

  async _yaesuSend(cmd) {
    // Yaesu simple command: 5 bytes, last byte is the command
    const frame = new Uint8Array([0x00, 0x00, 0x00, 0x00, cmd]);
    await this.writer.write(frame);
  }

  async _yaesuSetFreq(freqHz) {
    // Yaesu frequency format: 4 bytes BCD (8 digits), MSB first + cmd 0x01
    // e.g. 14.074000 MHz → 0x14 0x07 0x40 0x00
    let f = Math.round(freqHz / 10); // in 10 Hz units
    const bcd = new Uint8Array(5);
    for (let i = 3; i >= 0; i--) {
      const lo = f % 10; f = Math.floor(f / 10);
      const hi = f % 10; f = Math.floor(f / 10);
      bcd[i] = (hi << 4) | lo;
    }
    bcd[4] = 0x01; // Set Frequency command
    await this.writer.write(bcd);
  }

  // ── Icom CI-V protocol ────────────────────────────────────────────────

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
