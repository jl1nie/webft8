// QSO log — stores completed QSOs in localStorage.

const STORAGE_KEY = 'rs-ft8n-qso-log';

export class QsoLog {
  constructor() {
    this.entries = JSON.parse(localStorage.getItem(STORAGE_KEY) || '[]');
  }

  /**
   * Add a completed QSO.
   * @param {Object} qso
   * @param {string} qso.dxCall
   * @param {string} qso.dxGrid
   * @param {string} qso.txReport — report we sent
   * @param {string} qso.rxReport — report we received
   * @param {number} qso.freq — frequency in Hz
   */
  add(qso) {
    const entry = {
      utc: new Date().toISOString(),
      dxCall: qso.dxCall,
      dxGrid: qso.dxGrid || '',
      txReport: qso.txReport || '',
      rxReport: qso.rxReport || '',
      freq: qso.freq || 0,
    };
    this.entries.unshift(entry);
    this._save();
    return entry;
  }

  /** Get all entries (newest first). */
  getAll() { return this.entries; }

  /** Clear all entries. */
  clear() {
    this.entries = [];
    this._save();
  }

  /** Export as ADIF string. */
  toAdif() {
    let adif = 'ADIF Export from rs-ft8n\n<EOH>\n\n';
    for (const e of this.entries) {
      const d = new Date(e.utc);
      const date = d.toISOString().slice(0, 10).replace(/-/g, '');
      const time = d.toISOString().slice(11, 15).replace(/:/g, '');
      adif += `<CALL:${e.dxCall.length}>${e.dxCall}`;
      if (e.dxGrid) adif += ` <GRIDSQUARE:${e.dxGrid.length}>${e.dxGrid}`;
      adif += ` <MODE:3>FT8`;
      adif += ` <QSO_DATE:8>${date}`;
      adif += ` <TIME_ON:4>${time}`;
      if (e.rxReport) adif += ` <RST_RCVD:${e.rxReport.length}>${e.rxReport}`;
      if (e.txReport) adif += ` <RST_SENT:${e.txReport.length}>${e.txReport}`;
      adif += ` <EOR>\n`;
    }
    return adif;
  }

  _save() {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(this.entries));
  }
}
