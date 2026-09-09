// WSJT slot period manager. Tracks UTC-aligned periods and fires callbacks
// at boundaries. Supports TX queueing with even/odd slot control and
// automatic clock-offset correction via observed DT values.
//
// Slot length is configurable: 15 000 ms for FT8, 7 500 ms for FT4,
// 120 000 ms for WSPR. Name retained as `FT8PeriodManager` for historical
// call-sites; an alias `SlotPeriodManager` is exported for new code.

export class FT8PeriodManager {
  /**
   * @param {Object} callbacks
   * @param {function(number, boolean)} callbacks.onPeriodStart — (periodIndex, isEven) fires at period START
   * @param {function(number, boolean)} callbacks.onPeriodEnd — (periodIndex, isEven) fires at period END
   * @param {function(number)} callbacks.onTick — seconds remaining in current period
   * @param {number} [slotMs=15000] — period length in milliseconds (15 000 for FT8, 7 500 for FT4)
   */
  constructor(callbacks, slotMs = 15000) {
    this.callbacks = callbacks;
    this.slotMs = slotMs;
    this.slotSec = slotMs / 1000;
    this.tickInterval = null;
    this.boundaryTimeout = null;
    this.running = false;
    // True from the moment a boundary handler starts until it has re-armed
    // the next one — i.e. across the awaited decode. `setClockOffset` checks
    // it so a mid-decode offset update does not arm a competing timer.
    this._inBoundary = false;
    // Highest period index a boundary has actually been fired for, so an
    // early-firing setTimeout cannot re-run one.
    this._lastFiredPeriod = -Infinity;

    // TX queue: { call1, call2, report, freq, txEven }
    this.txQueue = null;
    // Period index when TX was queued — skip firing on the same boundary.
    this._txQueuedPeriod = -1;

    // ── DT auto-correction ──────────────────────────────────────────────────
    // clockOffsetMs: how much to delay the period boundary beyond the raw UTC
    // alignment.  Positive = our clock is fast (ahead) — we fire the boundary
    // later so the capture window slides right and signals appear near DT=0.
    //
    // Estimation: decoded DT values are accumulated each period.  After
    // MIN_SAMPLES are collected the median is used to update the offset.
    // The update is smoothed (EMA) to avoid jumps from spurious outliers.
    this.clockOffsetMs = 0;
    this._nextFireMs = 0;          // absolute ms when next boundary will fire (set by _scheduleBoundary)
    this._dtSamples = [];          // DT values collected this period
    this._dtHistory  = [];         // smoothed estimates, capped at HIST_LEN
    this._MIN_SAMPLES = 1;         // minimum decoded signals per period
    this._HIST_LEN   = 6;          // rolling history length (≈ 90 s)
    this._EMA_ALPHA  = 0.4;        // EMA smoothing factor
    this._dtAutoCorrect = true;    // FT8-signal-based correction enabled by default
  }

  /** Change the slot length on the fly (e.g. switching FT8 ↔ FT4). */
  setSlotMs(slotMs) {
    if (this.slotMs === slotMs) return;
    const wasRunning = this.running;
    if (wasRunning) this.stop();
    this.slotMs = slotMs;
    this.slotSec = slotMs / 1000;
    if (wasRunning) this.start();
  }

  start() {
    if (this.running) return;
    this.running = true;
    // Period indices are slot-length-relative, so they are not comparable
    // across a `setSlotMs()` restart (an FT8 index is ~half the FT4 index for
    // the same instant). Reset the high-water mark or switching to a longer
    // slot would leave every new index below it and wedge the boundary loop.
    this._lastFiredPeriod = -Infinity;
    this._inBoundary = false;
    this.tickInterval = setInterval(() => this._tick(), 100);
    this._scheduleBoundary();
  }

  stop() {
    this.running = false;
    if (this.tickInterval) { clearInterval(this.tickInterval); this.tickInterval = null; }
    if (this.boundaryTimeout) { clearTimeout(this.boundaryTimeout); this.boundaryTimeout = null; }
    this._inBoundary = false;
    this.txQueue = null;
  }

  getCurrentPeriod() {
    // Use UTC-corrected time so period boundaries align with actual UTC,
    // not with a potentially-drifted local clock.
    const nowUtc = Date.now() - this.clockOffsetMs;
    const periodIndex = Math.floor(nowUtc / this.slotMs);
    const isEven = periodIndex % 2 === 0;
    const periodStartMs = periodIndex * this.slotMs;
    const elapsed = (nowUtc - periodStartMs) / 1000;
    const remaining = this.slotSec - elapsed;
    return { periodIndex, isEven, elapsed, remaining };
  }

  /**
   * Queue a TX message for the next appropriate period.
   * @param {Object} tx — { call1, call2, report, freq }
   * @param {boolean|null} txEven — true=TX on even, false=odd, null=next period
   */
  queueTx(tx, txEven) {
    this.txQueue = { ...tx, txEven };
    this._txQueuedPeriod = this.getCurrentPeriod().periodIndex;
  }

  /** Cancel queued TX. */
  cancelTx() {
    this.txQueue = null;
  }

  /** Check if TX is queued. */
  hasTxQueued() {
    return this.txQueue !== null;
  }

  /**
   * Feed decoded DT values for clock-offset estimation.
   * Call once per period with the dt_sec of every successfully decoded signal.
   * @param {number[]} dtValues — array of dt_sec from decoded results
   */
  addDtSamples(dtValues) {
    this._dtSamples.push(...dtValues);
  }

  /** Return current clock offset estimate in seconds (positive = clock fast). */
  get clockOffsetSec() {
    return this.clockOffsetMs / 1000;
  }

  /**
   * Directly set the clock offset (e.g. from NTP measurement).
   * Overrides any FT8-signal-based estimate accumulated so far.
   * @param {number} offsetSec — positive = local clock is fast (ahead of UTC)
   */
  setClockOffset(offsetSec) {
    // Clamp to ±10 s (anything larger is likely a measurement error)
    const clamped = Math.max(-10, Math.min(10, offsetSec));
    this.clockOffsetMs = Math.round(clamped * 1000);
    // Discard any samples collected under the old timing — they're stale
    this._dtSamples = [];
    // Seed the FT8-based history so EMA starts from the new offset
    this._dtHistory = Array(this._HIST_LEN).fill(clamped);
    // Reschedule the pending boundary immediately so the new offset takes
    // effect from the very next period — not 1–2 periods later.
    //
    // Not while the boundary handler is mid-flight, though. This is called
    // from inside the awaited `onPeriodEnd` (app.js: `applyBootstrap()`, and
    // the NTP/GPS handlers can land there too), and that handler ends with a
    // `_scheduleBoundary()` of its own, which picks up the new offset anyway.
    // Scheduling here as well used to arm a *second*, unreferenced boundary
    // timer: `boundaryTimeout` was still holding the already-fired handle, so
    // the `clearTimeout` was a no-op and the assignment below was overwritten
    // moments later by the handler's own call. The result was two independent
    // boundary loops firing per slot — double decode, double `onPeriodStart`
    // (so a double WSPR beacon schedule), and a timer `stop()` could no
    // longer cancel. Measured on a 200 ms test slot: 25 boundaries where 10
    // were due.
    if (this.running && !this._inBoundary) {
      this._scheduleBoundary();
    }
    if (this.callbacks.onClockOffset) {
      this.callbacks.onClockOffset(clamped);
    }
  }

  /** Enable or disable FT8-signal-based DT auto-correction. */
  setDtAutoCorrect(enabled) {
    this._dtAutoCorrect = enabled;
  }

  /**
   * Single-shot cold-start bootstrap from `bootstrap_dt_median` (mfsk-core
   * 0.6.6). Use when no confirmed decode is available this period AND the
   * EMA has never been seeded — typically only the first 1-2 slots after
   * audio starts on a device whose clock is skewed >2 s from UTC.
   *
   * Ignored once the steady-state EMA has any samples — those are more
   * accurate than coarse_sync candidates and we don't want to clobber them.
   * Clamped to ±3 s (coarse_sync's native search lag is ±2.5 s; anything
   * larger is a false candidate).
   */
  applyBootstrap(estimateSec) {
    if (!Number.isFinite(estimateSec)) return;
    if (!this._dtAutoCorrect) return;
    if (this._dtHistory.length > 0) return;
    if (Math.abs(estimateSec) > 3) return;
    this.setClockOffset(estimateSec);
  }

  // ── Internal ────────────────────────────────────────────────────────────

  _tick() {
    // Use actual scheduled fire time so countdown reaches 0 exactly when the
    // boundary fires, regardless of clockOffsetMs direction or magnitude.
    const remaining = this._nextFireMs
      ? Math.max(0, (this._nextFireMs - Date.now()) / 1000)
      : Math.max(0, this.getCurrentPeriod().remaining);
    if (this.callbacks.onTick) {
      this.callbacks.onTick(remaining);
    }
  }

  /** Update clock offset from accumulated DT samples, then clear them. */
  _updateClockOffset() {
    const samples = this._dtSamples;
    this._dtSamples = [];
    if (!this._dtAutoCorrect) return;

    if (samples.length < this._MIN_SAMPLES) return;

    // Median DT of this period — robust to outliers from weak/partial decodes
    const sorted = [...samples].sort((a, b) => a - b);
    const median = sorted[Math.floor(sorted.length / 2)];

    // Clamp: ignore implausible values (> ±5 s are measurement errors)
    if (Math.abs(median) > 5) return;

    // Surface the raw per-period median DT (post-correction residual) so the
    // UI can flag ongoing drift. Fires BEFORE the EMA absorbs the value.
    if (this.callbacks.onPeriodDtMedian) {
      this.callbacks.onPeriodDtMedian(median);
    }

    // EMA update — smooths period-to-period jitter
    const prev = this._dtHistory.length > 0
      ? this._dtHistory[this._dtHistory.length - 1]
      : median;
    const smoothed = prev + this._EMA_ALPHA * (median - prev);

    this._dtHistory.push(smoothed);
    if (this._dtHistory.length > this._HIST_LEN) {
      this._dtHistory.shift();
    }

    // Use the mean of the recent history as the offset estimate.
    // decoded DT > 0  →  our clock is fast (ahead)  →  clockOffsetMs > 0
    const estimate = this._dtHistory.reduce((a, b) => a + b, 0) / this._dtHistory.length;
    this.clockOffsetMs = Math.round(estimate * 1000);

    if (this.callbacks.onClockOffset) {
      this.callbacks.onClockOffset(estimate);
    }
  }

  _scheduleBoundary() {
    if (!this.running) return;
    // Idempotent: never leave a previously-armed boundary running loose.
    // Every caller either has no timer pending or wants the pending one
    // replaced, and an orphaned timer is a second boundary loop (see
    // `setClockOffset`).
    if (this.boundaryTimeout) {
      clearTimeout(this.boundaryTimeout);
      this.boundaryTimeout = null;
    }
    const now = Date.now();
    // Always compute the current period in UTC time.
    // If the local clock is fast by N ms, Date.now() reads N ms ahead of UTC.
    // Using the raw Date.now() to compute the period index causes it to advance
    // early, targeting the wrong (next-next) boundary and firing 15 s late.
    const nowUtc = now - this.clockOffsetMs;
    const currentPeriod = Math.floor(nowUtc / this.slotMs);
    const nextBoundaryUtcMs = (currentPeriod + 1) * this.slotMs;
    // delay in local-clock ms: how long until the local clock reaches the
    // moment that corresponds to the next UTC boundary.
    const delay = Math.max(0, nextBoundaryUtcMs - nowUtc);
    this._nextFireMs = now + delay;  // track actual fire time for accurate countdown

    this.boundaryTimeout = setTimeout(async () => {
      // This handle has fired; it is no longer cancellable, so stop
      // presenting it as a pending timer to `stop()`/`_scheduleBoundary()`.
      this.boundaryTimeout = null;
      if (!this.running) return;

      const { periodIndex, isEven } = this.getCurrentPeriod();

      // setTimeout may fire a hair *early* (timer-vs-Date.now() skew, and
      // browsers are allowed to). `getCurrentPeriod()` then still reports the
      // period that is ending, so without this guard we would decode the
      // period before it, fire `onPeriodStart` on a stale index, and — since
      // the next boundary would compute a ~0 ms delay — immediately re-enter
      // in a tight loop. Just re-arm; the real boundary is a moment away.
      //
      // The other way in is a backwards clock-offset jump large enough to
      // cross a boundary (only reachable on the short slots: the ±10 s clamp
      // in `setClockOffset` cannot cross a 15 s FT8 period, but can cross a
      // 7.5 s FT4 one). That costs a skipped slot rather than a spin — the
      // re-arm below computes a full slot of delay, not zero — and recovers
      // by itself once the index passes the mark again.
      if (periodIndex <= this._lastFiredPeriod) {
        this._scheduleBoundary();
        return;
      }
      this._lastFiredPeriod = periodIndex;
      this._inBoundary = true;

      const endedPeriod = periodIndex - 1;
      const endedIsEven = endedPeriod % 2 === 0;

      // ── Fire TX FIRST, at the period boundary, before decode ──────────────
      // TX must start within ~2.4 s of the boundary (FT8 signal = 12.64 s;
      // must fit inside the 15 s receive window).  Decode can take 1–3 s,
      // so we fire TX immediately and decode concurrently.
      if (this.txQueue) {
        const { txEven } = this.txQueue;
        const slotMatch = txEven === null || txEven === isEven;
        const queuedThisBoundary = this._txQueuedPeriod === periodIndex;
        if (slotMatch && !queuedThisBoundary) {
          const tx = this.txQueue;
          this.txQueue = null;
          if (this.callbacks.onTxFire) {
            this.callbacks.onTxFire(tx);  // fire-and-forget (async TX)
          }
        }
      }

      // ── Period START callback ─────────────────────────────────────────────
      if (this.callbacks.onPeriodStart) {
        this.callbacks.onPeriodStart(periodIndex, isEven);
      }

      // ── Decode previous period (concurrently with TX) ─────────────────────
      if (this.callbacks.onPeriodEnd) {
        try {
          await this.callbacks.onPeriodEnd(endedPeriod, endedIsEven);
        } catch (e) {
          console.error('Decode error:', e);
        }
      }

      // ── Update clock offset from DT samples collected during decode ────────
      // Writes `clockOffsetMs` directly rather than going through
      // `setClockOffset`, so it does not try to reschedule from under us.
      this._updateClockOffset();

      this._inBoundary = false;
      this._scheduleBoundary();
    }, delay);
  }
}

// Backwards-compatible alias for code that imports by the newer name.
export { FT8PeriodManager as SlotPeriodManager };
