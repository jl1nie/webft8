// CAT (Computer Aided Transceiver) control via Web Serial / Web Bluetooth.
// Rig profiles loaded from rig-profiles.json (editable/extensible).

import { BleTransport } from './ble-transport.js';

let rigProfiles = {};

/** Load rig profiles from JSON file. */
export async function loadRigProfiles() {
  try {
    const url = new URL('rig-profiles.json', import.meta.url).href;
    const res = await fetch(url);
    rigProfiles = await res.json();
  } catch (e) {
    console.warn('Failed to load rig-profiles.json:', e);
  }
  return rigProfiles;
}

/** Get loaded profiles. */
export function getRigProfiles() { return rigProfiles; }

// ── Hex string helpers ──────────────────────────────────────────────────────

function hexToBytes(hexStr) {
  return new Uint8Array(hexStr.trim().split(/\s+/).map(h => parseInt(h, 16)));
}

function parseAddr(s) {
  if (typeof s === 'number') return s;
  return parseInt(s, 16);
}

// ── CAT Controller ──────────────────────────────────────────────────────────

export class CatController {
  constructor() {
    this.transport = null;  // { write(Uint8Array), disconnect() }
    this.transportType = ''; // 'serial' | 'ble'
    this.port = null;       // Web Serial port (serial mode only)
    this.writer = null;     // Web Serial writer (serial mode only)
    this.connected = false;
    this.rig = null;
    this.rigId = '';
    this.pttOn = false;
    this.narrowOn = false;
    this.onDisconnect = null;
  }

  static isSerialSupported() { return 'serial' in navigator; }
  static isBleSupported() { return BleTransport.isSupported(); }
  /** @deprecated Use isSerialSupported() */
  static isSupported() { return CatController.isSerialSupported(); }

  async requestPort() {
    if (!CatController.isSerialSupported()) throw new Error('Web Serial API not supported');
    this.port = await navigator.serial.requestPort();
    this.transportType = 'serial';
    return this.port;
  }

  async connectBle(rigId) {
    const rig = rigProfiles[rigId];
    if (!rig) throw new Error(`Unknown rig: ${rigId}`);
    if (!rig.ble) throw new Error(`${rig.label} does not support BLE`);

    const ble = new BleTransport();
    ble.onDisconnect = () => this._handleDisconnect();
    await ble.connect();

    this.transport = ble;
    this.transportType = 'ble';
    this.rig = rig;
    this.rigId = rigId;
    this.connected = true;
    this.pttOn = false;
    this.narrowOn = false;
  }

  async connect(rigId) {
    if (this.transportType === 'ble') {
      // BLE path — already connected via connectBle()
      return;
    }
    // Serial path
    if (!this.port) throw new Error('No port selected');
    const rig = rigProfiles[rigId];
    if (!rig) throw new Error(`Unknown rig: ${rigId}`);

    this.rig = rig;
    this.rigId = rigId;
    await this.port.open({ baudRate: rig.baud });
    this.writer = this.port.writable.getWriter();
    this.transportType = 'serial';
    this.transport = { write: (data) => this.writer.write(data) };
    this.connected = true;
    this.pttOn = false;
    this.narrowOn = false;

    if (this.port.readable) {
      this.port.readable.pipeTo(new WritableStream()).catch(() => this._handleDisconnect());
    }
  }

  async disconnect() {
    await this.safePttOff();
    if (this.transportType === 'ble') {
      if (this.transport) await this.transport.disconnect();
    } else {
      if (this.writer) { this.writer.releaseLock(); this.writer = null; }
      try { if (this.port) await this.port.close(); } catch (_) {}
    }
    this.transport = null;
    this.transportType = '';
    this.connected = false;
    this.pttOn = false;
    this.narrowOn = false;
  }

  async ptt(on) {
    if (!this.connected || !this.rig) return;
    try {
      const cmd = on ? this.rig.pttOn : this.rig.pttOff;
      if (this.rig.protocol === 'civ') {
        await this._civSendHex(cmd);
      } else {
        await this._sendText(cmd);
      }
      this.pttOn = on;
    } catch (e) {
      this._handleDisconnect();
      throw e;
    }
  }

  async safePttOff() {
    if (!this.connected || !this.pttOn) return;
    try { await this.ptt(false); } catch (_) { this.pttOn = false; }
  }

  async setFilter(narrow) {
    if (!this.connected || !this.rig) return;
    try {
      const cmd = narrow ? this.rig.filterNarrow : this.rig.filterWide;
      if (!cmd) return;
      if (this.rig.protocol === 'civ') {
        await this._civSendHex(cmd);
      } else {
        await this._sendText(cmd);
      }
      this.narrowOn = narrow;
    } catch (e) {
      this._handleDisconnect();
    }
  }

  async setFreq(freqHz) {
    if (!this.connected || !this.rig) return;
    try {
      if (this.rig.protocol === 'civ') {
        await this._civSetFreq(freqHz);
      } else {
        const hz = String(Math.round(freqHz)).padStart(9, '0');
        await this._sendText(`FA${hz};`);
      }
    } catch (e) {
      this._handleDisconnect();
    }
  }

  // ── Internal ──────────────────────────────────────────────────────────

  _handleDisconnect() {
    this.connected = false;
    this.pttOn = false;
    this.narrowOn = false;
    if (this.onDisconnect) this.onDisconnect();
  }

  async _sendText(cmd) {
    await this.transport.write(new TextEncoder().encode(cmd));
  }

  async _civSendHex(hexStr) {
    const addr = parseAddr(this.rig.civAddr || '0x94');
    const data = hexToBytes(hexStr);
    const frame = new Uint8Array([0xFE, 0xFE, addr, 0xE0, ...data, 0xFD]);
    await this.transport.write(frame);
  }

  async _civSetFreq(freqHz) {
    const bcd = [];
    let f = Math.round(freqHz);
    for (let i = 0; i < 5; i++) {
      const lo = f % 10; f = Math.floor(f / 10);
      const hi = f % 10; f = Math.floor(f / 10);
      bcd.push((hi << 4) | lo);
    }
    const addr = parseAddr(this.rig.civAddr || '0x94');
    const frame = new Uint8Array([0xFE, 0xFE, addr, 0xE0, 0x05, ...bcd, 0xFD]);
    await this.transport.write(frame);
  }
}
