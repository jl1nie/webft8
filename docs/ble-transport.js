// BLE Transport for Icom IC-705 CI-V over Web Bluetooth API.
// GATT protocol based on K7MDL2/IC-705-BLE-Serial-Example.

const SERVICE_UUID = '14cf8001-1ec2-d408-1b04-2eb270f14203';
const CHAR_UUID    = '14cf8002-1ec2-d408-1b04-2eb270f14203';

// Pairing constants
const DEVICE_UUID = '00001101-0000-1000-8000-00805F9B34FB'; // 36 chars ASCII
const DEVICE_NAME = 'rs-ft8n BLE\x00\x00\x00\x00\x00';      // 16 chars padded
const PAIR_TOKEN  = [0xEE, 0x39, 0x09, 0x10];

function delay(ms) { return new Promise(r => setTimeout(r, ms)); }

export class BleTransport {
  constructor() {
    this.device = null;
    this.server = null;
    this.char = null;
    this.connected = false;
    this.onDisconnect = null;
    this._grantResolve = null;
    this._pairState = { uuid: false, name: false, token: false, granted: false };
  }

  static isSupported() {
    return typeof navigator !== 'undefined' && 'bluetooth' in navigator;
  }

  async connect() {
    // Request device — IC-705 may not include service UUID in advertisement,
    // so filter by name prefix and declare the service as optional.
    this.device = await navigator.bluetooth.requestDevice({
      filters: [
        { namePrefix: 'ICOM' },
        { namePrefix: 'IC-705' },
        { services: [SERVICE_UUID] },
      ],
      optionalServices: [SERVICE_UUID],
    });

    this.device.addEventListener('gattserverdisconnected', () => {
      this.connected = false;
      if (this.onDisconnect) this.onDisconnect();
    });

    // Connect GATT
    this.server = await this.device.gatt.connect();
    const service = await this.server.getPrimaryService(SERVICE_UUID);
    this.char = await service.getCharacteristic(CHAR_UUID);

    // Enable notifications for receiving responses
    this.char.addEventListener('characteristicvaluechanged', (e) => this._onNotify(e));
    await this.char.startNotifications();

    // Run pairing sequence
    await this._pair();
    this.connected = true;
  }

  async write(data) {
    if (!this.char) throw new Error('BLE not connected');
    // data is Uint8Array
    await this.char.writeValueWithoutResponse(data);
  }

  async disconnect() {
    this.connected = false;
    try {
      if (this.device && this.device.gatt.connected) {
        this.device.gatt.disconnect();
      }
    } catch (_) {}
    this.device = null;
    this.server = null;
    this.char = null;
  }

  // ── Internal ────────────────────────────────────────────────────────────

  _onNotify(event) {
    const data = new Uint8Array(event.target.value.buffer);
    if (data.length < 4) return;

    // Pairing response: FE F1 00 xx [data] FD
    if (data[0] === 0xFE && data[1] === 0xF1 && data[2] === 0x00) {
      const code = data[3];
      if (code === 0x61) this._pairState.uuid = true;
      else if (code === 0x62) this._pairState.name = true;
      else if (code === 0x63) this._pairState.token = true;
      else if (code === 0x64) {
        this._pairState.granted = true;
        if (this._grantResolve) this._grantResolve();
      }
    }
    // CI-V responses (FE FE ...) are ignored for now — write-only mode
  }

  async _pair() {
    this._pairState = { uuid: false, name: false, token: false, granted: false };

    // Message 1: UUID (41 bytes)
    const uuidBytes = new TextEncoder().encode(DEVICE_UUID);
    const msg1 = new Uint8Array([0xFE, 0xF1, 0x00, 0x61, ...uuidBytes, 0xFD]);
    await this.char.writeValueWithoutResponse(msg1);
    await delay(20);

    // Message 2: Device name (21 bytes)
    const nameBytes = new TextEncoder().encode(DEVICE_NAME);
    const msg2 = new Uint8Array([0xFE, 0xF1, 0x00, 0x62, ...nameBytes, 0xFD]);
    await this.char.writeValueWithoutResponse(msg2);
    await delay(20);

    // Message 3: Token (9 bytes)
    const msg3 = new Uint8Array([0xFE, 0xF1, 0x00, 0x63, ...PAIR_TOKEN, 0xFD]);
    await this.char.writeValueWithoutResponse(msg3);

    // Wait for CI-V bus access grant (0x64)
    if (!this._pairState.granted) {
      await new Promise((resolve, reject) => {
        this._grantResolve = resolve;
        setTimeout(() => reject(new Error('BLE pairing timeout (no CI-V grant)')), 5000);
      });
    }
  }
}
