// FT8 15-second period manager.
// Tracks UTC-aligned periods and fires callbacks at boundaries.

export class FT8PeriodManager {
  /**
   * @param {Object} callbacks
   * @param {function(number, boolean)} callbacks.onPeriodEnd - (periodIndex, isEven)
   * @param {function(number)} callbacks.onTick - seconds remaining in current period
   */
  constructor(callbacks) {
    this.callbacks = callbacks;
    this.tickInterval = null;
    this.boundaryTimeout = null;
    this.running = false;
  }

  /** Start period tracking. */
  start() {
    if (this.running) return;
    this.running = true;

    // Tick every 100ms for countdown display
    this.tickInterval = setInterval(() => this._tick(), 100);

    // Schedule first boundary
    this._scheduleBoundary();
  }

  /** Stop period tracking. */
  stop() {
    this.running = false;
    if (this.tickInterval) { clearInterval(this.tickInterval); this.tickInterval = null; }
    if (this.boundaryTimeout) { clearTimeout(this.boundaryTimeout); this.boundaryTimeout = null; }
  }

  /** Get current period info. */
  getCurrentPeriod() {
    const now = Date.now();
    const periodIndex = Math.floor(now / 15000);
    const isEven = periodIndex % 2 === 0;
    const periodStartMs = periodIndex * 15000;
    const elapsed = (now - periodStartMs) / 1000;
    const remaining = 15 - elapsed;
    return { periodIndex, isEven, elapsed, remaining };
  }

  // ── Internal ────────────────────────────────────────────────────────────

  _tick() {
    const { remaining } = this.getCurrentPeriod();
    if (this.callbacks.onTick) {
      this.callbacks.onTick(Math.max(0, remaining));
    }
  }

  _scheduleBoundary() {
    if (!this.running) return;
    const now = Date.now();
    const currentPeriod = Math.floor(now / 15000);
    const nextBoundaryMs = (currentPeriod + 1) * 15000;
    const delay = nextBoundaryMs - now;

    this.boundaryTimeout = setTimeout(() => {
      if (!this.running) return;
      const { periodIndex, isEven } = this.getCurrentPeriod();
      // Fire end of the period that just completed
      if (this.callbacks.onPeriodEnd) {
        this.callbacks.onPeriodEnd(periodIndex - 1, (periodIndex - 1) % 2 === 0);
      }
      // Schedule next
      this._scheduleBoundary();
    }, delay);
  }
}
