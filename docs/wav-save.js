// wav-save.js — save received audio slots as 12 kHz mono 16-bit WAV.
//
// "Save All" style recording: every live RX period is written to a
// user-chosen folder via the File System Access API (Chrome/Edge — the
// same platform WebFT8's Web Serial / Web Bluetooth features already
// require). The directory handle is persisted in IndexedDB so it
// survives a reload; the browser still requires a fresh read-write
// permission grant per session, which we request on a user gesture
// (folder button / Start Audio / enabling the toggle).
//
// Files are named WSJT-X style: YYMMDD_HHMMSS.wav (UTC slot start).

const DB_NAME = 'webft8-wav';
const STORE = 'handles';
const KEY = 'dir';

// ── WAV encoding ────────────────────────────────────────────────────────
// Encode Float32 PCM in [-1, 1] to a 16-bit mono WAV ArrayBuffer.
export function encodeWav(float32, sampleRate) {
  const n = float32.length;
  const buf = new ArrayBuffer(44 + n * 2);
  const dv = new DataView(buf);
  const wstr = (o, s) => { for (let i = 0; i < s.length; i++) dv.setUint8(o + i, s.charCodeAt(i)); };
  wstr(0, 'RIFF');
  dv.setUint32(4, 36 + n * 2, true);
  wstr(8, 'WAVE');
  wstr(12, 'fmt ');
  dv.setUint32(16, 16, true);          // fmt chunk size
  dv.setUint16(20, 1, true);           // PCM
  dv.setUint16(22, 1, true);           // mono
  dv.setUint32(24, sampleRate, true);
  dv.setUint32(28, sampleRate * 2, true); // byte rate (sr * blockAlign)
  dv.setUint16(32, 2, true);           // block align (ch * bytes)
  dv.setUint16(34, 16, true);          // bits per sample
  wstr(36, 'data');
  dv.setUint32(40, n * 2, true);
  let o = 44;
  for (let i = 0; i < n; i++) {
    let s = float32[i];
    if (s > 1) s = 1; else if (s < -1) s = -1;
    dv.setInt16(o, s < 0 ? s * 0x8000 : s * 0x7fff, true);
    o += 2;
  }
  return buf;
}

// WSJT-X-style UTC filename: YYMMDD_HHMMSS.wav
export function wavFilename(date) {
  const p = (x) => String(x).padStart(2, '0');
  return `${p(date.getUTCFullYear() % 100)}${p(date.getUTCMonth() + 1)}${p(date.getUTCDate())}`
       + `_${p(date.getUTCHours())}${p(date.getUTCMinutes())}${p(date.getUTCSeconds())}.wav`;
}

// ── IndexedDB (directory-handle persistence) ────────────────────────────
function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}
async function idbPut(handle) {
  const db = await openDb();
  await new Promise((res, rej) => {
    const tx = db.transaction(STORE, 'readwrite');
    tx.objectStore(STORE).put(handle, KEY);
    tx.oncomplete = res;
    tx.onerror = () => rej(tx.error);
  });
  db.close();
}
async function idbGet() {
  const db = await openDb();
  const h = await new Promise((res, rej) => {
    const tx = db.transaction(STORE, 'readonly');
    const r = tx.objectStore(STORE).get(KEY);
    r.onsuccess = () => res(r.result || null);
    r.onerror = () => rej(r.error);
  });
  db.close();
  return h;
}

// ── Saver ───────────────────────────────────────────────────────────────
export class WavSaver {
  constructor() {
    this.dirHandle = null;
  }

  static get supported() {
    return typeof window !== 'undefined' && 'showDirectoryPicker' in window;
  }

  get folderName() {
    return this.dirHandle ? this.dirHandle.name : null;
  }

  /** Restore a previously chosen folder handle from IndexedDB (no prompt). */
  async restore() {
    try {
      const h = await idbGet();
      if (h) this.dirHandle = h;
    } catch { /* IndexedDB unavailable — non-fatal */ }
    return this.folderName;
  }

  /** Prompt the user to pick a folder (must run in a user gesture). */
  async pickFolder() {
    if (!WavSaver.supported) throw new Error('File System Access API unavailable (use Chrome/Edge)');
    const h = await window.showDirectoryPicker({ id: 'webft8-wav', mode: 'readwrite' });
    this.dirHandle = h;
    try { await idbPut(h); } catch { /* persistence optional */ }
    return h.name;
  }

  /**
   * Ensure read-write permission on the current folder. Returns true when
   * writable. queryPermission is silent; requestPermission needs a user
   * gesture, so call this from a click / change handler.
   */
  async ensureWritable() {
    if (!this.dirHandle) return false;
    const opts = { mode: 'readwrite' };
    if ((await this.dirHandle.queryPermission(opts)) === 'granted') return true;
    return (await this.dirHandle.requestPermission(opts)) === 'granted';
  }

  /**
   * Write one slot to `<folder>/YYMMDD_HHMMSS.wav`. `date` is the slot
   * start (UTC). Assumes permission is already granted (ensureWritable).
   */
  async save(float32, sampleRate, date) {
    if (!this.dirHandle) throw new Error('No folder chosen');
    const name = wavFilename(date);
    const fh = await this.dirHandle.getFileHandle(name, { create: true });
    const w = await fh.createWritable();
    await w.write(encodeWav(float32, sampleRate));
    await w.close();
    return name;
  }
}
