// GPS NMEA UTC sync via Web Serial API.
//
// Reads $GNRMC / $GPRMC sentences from IC-705 USB-B GPS port (9600 baud)
// and derives a clock-offset estimate for FT8 period synchronisation.
//
// Usage:
//   const sync = new GpsNmeaSync((offsetSec, label) => {
//     periodMgr.setClockOffset(offsetSec);
//   });
//   await sync.connect();   // shows browser port-picker dialog
//   // ...
//   await sync.disconnect();

/**
 * Parse NMEA UTC time + date into a UTC epoch (ms).
 *
 * @param {string} timeStr  HHMMSS or HHMMSS.ss
 * @param {string} dateStr  DDMMYY  (optional — used only for day-boundary check)
 * @param {number} referenceMs  Date.now() at receive time (for day-boundary fix)
 * @returns {number|null}  UTC milliseconds, or null if unparseable
 */
function parseNmeaUtcMs(timeStr, dateStr, referenceMs) {
  if (!timeStr || timeStr.length < 6) return null;
  const hh = parseInt(timeStr.substr(0, 2), 10);
  const mm = parseInt(timeStr.substr(2, 2), 10);
  const ss = parseFloat(timeStr.substr(4));
  if (isNaN(hh) || isNaN(mm) || isNaN(ss)) return null;

  // Build UTC ms from the reference date (day) + NMEA time-of-day
  const ref = new Date(referenceMs);
  const gpsMs = Date.UTC(
    ref.getUTCFullYear(), ref.getUTCMonth(), ref.getUTCDate(),
    hh, mm, 0, 0
  ) + Math.round(ss * 1000);

  // Day-boundary correction: if difference > ±12 h, shift by ±1 day
  const diff = referenceMs - gpsMs;
  if (diff >  43200000) return gpsMs + 86400000;
  if (diff < -43200000) return gpsMs - 86400000;
  return gpsMs;
}

/** Verify NMEA checksum (*XX at end of sentence). */
function nmeaChecksumOk(line) {
  const star = line.lastIndexOf('*');
  if (star < 0 || star + 3 > line.length) return false;
  let cs = 0;
  for (let i = 1; i < star; i++) cs ^= line.charCodeAt(i);
  return cs === parseInt(line.substr(star + 1, 2), 16);
}

export class GpsNmeaSync {
  /** Returns true if Web Serial API is available in this browser. */
  static isSupported() {
    return typeof navigator !== 'undefined' && 'serial' in navigator;
  }

  /**
   * @param {function(number, string)} onSync
   *   Called with (offsetSec, label) each time a valid UTC fix is received.
   *   offsetSec: positive = local clock fast (ahead of GPS UTC).
   */
  constructor(onSync) {
    this.onSync = onSync;
    this._port = null;
    this._running = false;
  }

  /**
   * Open the GPS serial port (shows browser port-picker dialog).
   * Resolves when the port is open and the read loop has started.
   */
  async connect() {
    this._port = await navigator.serial.requestPort();
    await this._port.open({ baudRate: 9600 });
    this._running = true;
    this._readLoop();  // fire-and-forget async loop
  }

  /** Stop reading and close the port. */
  async disconnect() {
    this._running = false;
    try { await this._port?.close(); } catch (_) {}
    this._port = null;
  }

  // ── Internal ────────────────────────────────────────────────────────────

  async _readLoop() {
    const decoder = new TextDecoder();
    let buf = '';
    let reader;
    try {
      reader = this._port.readable.getReader();
      while (this._running) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        // Split on newline; keep incomplete last chunk in buf
        const lines = buf.split('\n');
        buf = lines.pop();
        for (const line of lines) {
          this._parseLine(line.trim());
        }
      }
    } catch (e) {
      if (this._running) console.warn('GPS NMEA read error:', e);
    } finally {
      try { reader?.releaseLock(); } catch (_) {}
    }
  }

  _parseLine(line) {
    if (!line.startsWith('$GNRMC') && !line.startsWith('$GPRMC')) return;
    if (!nmeaChecksumOk(line)) return;

    const fields = line.split(',');
    // fields[2]: A = valid fix, V = void — skip void fixes
    if (fields[2] !== 'A') return;

    const receiveMs = Date.now();
    const gpsMs = parseNmeaUtcMs(fields[1], fields[9], receiveMs);
    if (!gpsMs) return;

    // IC-705 GPS sends NMEA at 9600 baud; a ~75-char sentence takes ≈78 ms
    // to transmit.  The sentence starts at the GPS second boundary, so we
    // subtract a fixed latency to get a better estimate of when that second
    // actually began.
    const NMEA_LATENCY_MS = 80;
    const offsetSec = (receiveMs - NMEA_LATENCY_MS - gpsMs) / 1000;
    if (Math.abs(offsetSec) > 10) return;  // implausible

    this.onSync(offsetSec, 'GPS-NMEA');
  }
}
