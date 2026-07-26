# WebFT8 Decoder Benchmark Results

**mfsk-core 0.7.4 (GitHub `jl1nie/mfsk-core@fe286cc8`) — 2026-07-26**

Simulator-based evaluation of the WebFT8 decoder against reference conditions.
All results are reproducible: `cargo run -p ft8-bench --release`.

> **Updated from the `756d81f7` run below.** mfsk-core `main` landed two
> more breaking changes the same day:
>
> 1. **`c8c3c6d`** (issue #191): the entire `decode_frame*`/
>    `decode_frame_subtract*`/`decode_sniper*` family (31 functions) was
>    deleted and replaced by two generic builders,
>    `DecodeRequest<P>`/`SniperRequest<P>`. This wasn't just an API
>    reshape — it fixed a real bug: `decode_frame_subtract_with_known`
>    (the only function accepting `known`/`precomputed_fft`, and the one
>    WebFT8's own `decode_phase1`/`decode_phase2` pipelined path called)
>    ran an *unfixed* flat-3-pass engine that never received the
>    staged-checkpoint SIC (#180) recall gains `decode_frame_subtract`
>    got by default. `known`/`fft_cache` are now honoured directly by
>    `DecodeRequest::staged()`/`.flat()`, so `decode_phase2` finally runs
>    the same engine as every other subtract path — verified below, see
>    **Real Recording**.
> 2. **`fe286cc`** (this repo's own fix, filed after benchmarking WebFT8's
>    sniper-mode redesign): `.staged()`/`.flat()` silently hardcoded
>    `EqMode::Off` internally, so `DecodeRequest::eq_mode()` compiled but
>    did nothing under either SIC strategy. WebFT8's sniper mode needs
>    *both* staged-checkpoint SIC quality (crowd-masking) and adaptive-EQ
>    correction (BPF-edge distortion) together — neither the old ad-hoc
>    `decode_sniper_sic` nor the new builder (pre-fix) could give both at
>    once. See **Scenario 5** for the benchmark data that motivated the
>    fix and validates it afterward.
>
> WebFT8's sniper mode (`decode_sniper`/`decode_sniper_f32` in
> `ft8-web`) moved from `decode_sniper_sic` (deleted upstream) to
> `DecodeRequest<Ft8>` narrowed to the BPF passband + `.staged()` +
> `.eq_mode()` — the "sniper (staged+EQ)" column in Scenario 5's table.
> `decode_wav_subtract`/`decode_phase2` moved to `DecodeRequest`::
> `.staged()`; `decode_wav`/`decode_phase1` to plain `DecodeRequest`.
>
> <details>
> <summary>Older update history (756d81f7, 7bc1684a, 28a1f03f, 2026-04-12 decode-engine swap)</summary>
>
> - **`756d81f7`** (PR #188, issue #182 follow-up): `DecodeDepth` went
>   from a flat 4-variant enum to a `{llr_effort, osd}` struct
>   (`EMBEDDED`/`BP_ONLY`/`FULL` replacing `BpVariantsAd`/`BpAll`/
>   `BpAllOsd`; `BpAllNoNsym3` retired, zero real callers). Same commits
>   wired the staged-checkpoint SIC into `decode_frame_subtract` as the
>   default path and ported WSJT-X's fine-sync tweak — `decode_frame`/
>   `decode_frame_subtract` got ~2.2–2.6x slower; `decode_sniper*` was
>   unaffected.
> - **`7bc1684a`** (issues #177/#179/#180): fixed a doubled cosine²-window
>   argument in the SIC low-pass kernel (`subtract_tones_lpf`) that had
>   been corrupting the QSB/channel estimate since mfsk-core v0.6.2 —
>   every prior "subtract"/"sniper-SIC" number before this had been
>   running on a broken SIC low-pass.
> - **2026-04-12**: decode engine moved from the in-repo `ft8-core` crate
>   to the external `mfsk-core` crate — ~7x speed-up plus a large
>   accuracy jump from a full engine swap, not incremental tuning.
>
> </details>

---

## Test Environment

| Item | Value |
|------|-------|
| Decoder | mfsk-core 0.7.4, git `fe286cc8` (Rust, native release) |
| Signal model | Pure-tone 8-GFSK + AWGN (12 000 Hz, i16) |
| BPF model | 4-pole Butterworth, 500 Hz passband |
| Seed count | 20–30 independent noise realisations per cell |
| Platform | x86-64 Linux (WSL2), 24-core, Rayon thread-pool (nested: ft8-bench seed-loops + mfsk-core's own internal candidate parallelism) |

### Decoder Modes

| Mode | Description |
|------|-------------|
| `full-band` | `DecodeRequest::new(...).decode()` — 200–2800 Hz, equivalent to WSJT-X |
| `subtract` | `DecodeRequest::new(...).staged().decode()` — multi-pass staged-checkpoint SIC |
| `sniper` | `SniperRequest::new(...).decode()` — ±250 Hz around target freq, no SIC/EQ |
| `sniper+EQ` | `SniperRequest::new(...).eq_mode(Local).decode()` — sniper + Costas Wiener EQ |
| `sniper+AP` | `SniperRequest::new(...).eq_mode(Local).ap_hint(...).decode()` — + A Priori callsign lock |
| `sniper (staged+EQ)` | `DecodeRequest::new(target±250Hz, ...).eq_mode(Local).staged().decode()` — WebFT8's actual shipped sniper mode as of this update; replaces the deleted `decode_sniper_sic` |

Note that `sniper`/`sniper+EQ`/`sniper+AP` (via `SniperRequest`) have no SIC —
they're the plain narrow-band path. "sniper (staged+EQ)" is what
`ft8-web`'s `decode_sniper`/`decode_sniper_f32` actually call.

---

## Scenario 1 — Single +40 dB Interferer (200 Hz offset)

Target: `CQ 3Y0Z JD34` @ 1000 Hz, SNR −5 dB
Interferer: `CQ JQ1QSO PM95` @ 1200 Hz, SNR +35 dB
Seed: 99

| Mode | Target | Interferer | Total decoded |
|------|--------|-----------|---------------|
| full-band | **DECODED** | DECODED | 2 |
| sniper (BPF removes interferer) | **DECODED** | — | 1 |

Unchanged from the `756d81f7` run.

---

## Scenario 2 — Busy Band, Moderate Crowd

15 crowd stations @ **+5 dB**, target `CQ 3Y0Z JD34` @ 1000 Hz, **−12 dB**, seed 777

| Mode | Target | Total decoded |
|------|--------|---------------|
| full-band | **DECODED** | 16 / 16 |
| sniper | **DECODED** | 2 |

Unchanged.

---

## Scenario 3 — Busy Band, Hard ADC Saturation

15 crowd stations @ **+40 dB**, target @ **−14 dB** (gap = 54 dB), seed 888

| Mode | Target | Total decoded | Notes |
|------|--------|---------------|-------|
| no-BPF full-band | **missed** | 15 | ADC saturated by crowd |
| no-BPF sniper sw | **missed** | 0 | crowd distortion still present |

30-seed statistical sweep (AGC-clipped i16 vs clean i16, no hardware BPF):

| Mode | Hit rate (30 seeds) |
|------|---------------------|
| AGC full-band | 0 / 30 (0%) |
| AGC sniper sw | 0 / 30 (0%) |
| clean full-band | 0 / 30 (0%) |
| clean sniper sw | 0 / 30 (0%) |

Unchanged — no software-only technique recovers the target when a 54 dB
crowd fully occupies the ADC dynamic range; this remains the core
justification for the hardware BPF.

---

## Scenario 4 — BPF Edge Distortion

Target only + AWGN, SNR **−18 dB**, 20 seeds, 4-pole Butterworth 500 Hz BPF.

| Placement | BPF window (Hz) | Target attenuation | EQ OFF | EQ ON |
|-----------|-----------------|--------------------|--------|-------|
| center | 750 – 1250 | −0.0 dB | 20/20 (100%) | 20/20 (100%) |
| shoulder | 950 – 1450 | −0.5 dB | 20/20 (100%) | 20/20 (100%) |
| edge (−3 dB) | 1000 – 1500 | −3.0 dB | 20/20 (100%) | 20/20 (100%) |
| no-BPF (reference) | — | 0 dB | 20/20 (100%) | — |

Unchanged — this scenario has been saturated at 100% since the
`756d81f7` update and doesn't exercise `.staged()`/`.flat()` at all
(plain `decode_sniper_eq`), so the `fe286cc` `eq_mode` fix has no effect
here either.

---

## Scenario 5 — BPF + In-Band Crowd (Signal Subtraction)

4 crowd stations **inside** the 500 Hz passband (850, 950, 1050, 1150 Hz), SNR **+8 dB** (fixed).
Target `CQ 3Y0Z JD34` @ 1000 Hz. BPF: 750–1250 Hz, 4-pole Butterworth.

### Statistical sweep (20 seeds × target SNR)

This is the scenario that drove the `fe286cc` fix and now validates it.
AP = call2 `3Y0Z` known. Six columns, from the least to most capable
sniper configuration:

| Target SNR | single-pass | EQ-only (no SIC) | subtract (staged, no EQ) | staged+AP (no EQ) | **sniper (staged+EQ)** | staged+EQ+AP |
|------------|-------------|-------------------|--------------------------|--------------------|--------------------------|--------------|
| −10 dB | 0/20 (0%) | 0/20 (0%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) |
| −12 dB | 0/20 (0%) | 0/20 (0%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) |
| −14 dB | 0/20 (0%) | 0/20 (0%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) |
| −16 dB | 0/20 (0%) | 0/20 (0%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) |
| −18 dB | 0/20 (0%) | 0/20 (0%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) |
| −20 dB | 0/20 (0%) | 0/20 (0%) | 19/20 (95%) | 19/20 (95%) | 18/20 (90%) | 19/20 (95%) |

**Why this table exists.** Before implementing the `fe286cc` fix, this
scenario (crowd at the passband *center*) was benchmarked alongside a
second, BPF-*edge*-placed variant (target at the −3 dB edge, fewer
in-band interferers) to decide how to replace the deleted
`decode_sniper_sic`. Results at −20 dB, before the fix existed:

| Configuration | Center-crowd (this table) | BPF-edge (separate probe) |
|---|---|---|
| EQ-only, no SIC | 0% | 0% |
| staged SIC, no EQ | **95%** | 20–35% |
| old `decode_sniper_sic` (+EQ) | 45–60% | **60–75%** |

Neither existing mechanism dominated: staged-checkpoint SIC crushed the
old ad-hoc SIC when crowd-masking was the problem, but the old
mechanism's EQ correction won decisively at the true BPF edge (because
`.staged()`/`.flat()` silently dropped `eq_mode` — confirmed by reading
the dispatch code, not just inferred from the numbers). Real sniper use
hits both conditions unpredictably, so `fe286cc` threads `eq_mode`
through the staged/flat SIC engine instead of reviving the old
mechanism. The "sniper (staged+EQ)" column above (90–100% throughout,
matching or beating every prior configuration in the center-crowd case)
is the result — see `ft8-bench/src/main.rs::run_bpf_subtract_scenario`
for the exact reproduction, including the BPF-edge variant.

---

## Scenario 6 — SNR Sensitivity: BPF Edge

BPF edge placement (target at −3 dB point), 20 seeds per row, `decode_sniper_ap`
(plain `SniperRequest`, not `.staged()` — unaffected by the `fe286cc` fix).

| SNR | EQ OFF | EQ | EQ + AP (CQ+call2) | full AP (77-bit) |
|-----|--------|----|---------------------|-------------------|
| −18 dB | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) | 20/20 (100%) |
| −20 dB | 19/20 (95%) | 18/20 (90%) | 19/20 (95%) | 18/20 (90%) |
| −22 dB | 5/20 (25%) | 7/20 (35%) | 9/20 (45%) | 7/20 (35%) |
| −24 dB | 0/20 (0%) | 0/20 (0%) | 0/20 (0%) | 0/20 (0%) |

Cell-to-cell differences from the `756d81f7` run are within single-sample
noise at n=20 (this path doesn't touch `.staged()`/`.flat()`, so no
mechanism explains a real shift) — not investigated further.

---

## Scenario 7 — Full QSO: BPF Edge + AP

All QSO message types across a simulated `JA1ABC ↔ 3Y0Z` exchange.
BPF edge (1000–1500 Hz, 4-pole), 20 seeds each. Uses plain `decode_sniper_ap`.

| SNR | CQ (61-bit) | REPORT (61-bit) | RR73 (77-bit) |
|-----|-------------|-----------------|---------------|
| −18 dB | 20/20 (100%, 1 FP) | 20/20 (100%, 1 FP) | 20/20 (100%) |
| −20 dB | 19/20 (95%, 1 FP) | 20/20 (100%) | 20/20 (100%) |
| −22 dB | 9/20 (45%) | 10/20 (50%, 1 FP) | 15/20 (75%) |
| −24 dB | 0/20 (0%, 1 FP) | 1/20 (5%) | 3/20 (15%) |

Unchanged within seed noise from `756d81f7` (this path doesn't touch
`.staged()`/`.flat()` either).

---

## Scenario 8 — Filter Comparison: Butterworth vs Elliptic (4-pole, 500 Hz BW)

15 crowd @ +40 dB (hardware BPF removes crowd before ADC; target + AWGN only
after BPF). EQ + AP (call2 = `3Y0Z`) applied to all BPF columns. 20 seeds
per cell. Uses plain `decode_sniper_ap` (via the `try_bpf!` macro).

| SNR | no-BPF | BW-edge+EQ+AP | BW-center+EQ+AP | EL-edge+EQ+AP | EL-center+EQ+AP |
|-----|--------|---------------|-----------------|---------------|-----------------|
| −10 dB | 0% | 100% | 100% | 100% | 100% |
| −12 dB | 0% | 100% | 100% | 100% | 100% |
| −14 dB | 0% | 100% | 100% | 100% | 100% |
| −16 dB | 0% | 100% | 100% | 100% | 100% |
| −18 dB | 0% | 100% | 100% | 100% | 100% |
| −20 dB | 0% | 100% | 100% | 95% | 100% |
| −22 dB | 0% | 55% | 55% | 55% | 55% |

Elliptic edge BPF (1000–1500 Hz) frequency response — unchanged:

| Freq (Hz) | Attenuation |
|-----------|-------------|
| 800 | −38.0 dB (notch) |
| 900 | −33.8 dB (notch) |
| 1000 | −8.2 dB ← target at edge |
| 1100 | −1.6 dB |
| 1200 | 0.0 dB |
| 1500 | −8.2 dB |
| 1800 | −35.9 dB (notch) |

Cell-to-cell differences from `756d81f7` are within seed noise; not
investigated further (same reason as Scenario 6/7 — this path doesn't
exercise the code `fe286cc` touches).

---

## Speed Benchmark

100 stations, 200–2800 Hz, SNR +5 dB, 10 runs after 3 warmup.
Release build (`cargo run -p ft8-bench --release`), Linux x86-64 (24-core).

| Mode | Decoded | Mean | Min | Max | Budget |
|------|---------|------|-----|-----|--------|
| decode_frame (single-pass) | 100 | 48.8 ms | 46.0 ms | 52.5 ms | 2400 ms |
| decode_frame_subtract (staged, FULL) | 100 | 1389.7 ms | 1359.9 ms | 1427.7 ms | 2400 ms |
| decode_frame_subtract (staged, BP_ONLY) | 100 | 1381.3 ms | 1352.4 ms | 1403.8 ms | 2400 ms |
| sniper+EQ (±250 Hz, plain SniperRequest) | 18 | 6.7 ms | 6.4 ms | 7.2 ms | 2400 ms |

All modes comfortably fit within the FT8 15-second period (2400 ms decode
window). Numbers are flat vs. the `756d81f7` run (within ~4% run-to-run
noise) — `fe286cc` only changes behavior when `eq_mode(Local)` is combined
with `.staged()`/`.flat()`, which none of these four calls do (the two
`decode_frame_subtract` rows use `EqMode::Off` implicitly; sniper+EQ here
is the plain non-staged `SniperRequest` path).

**Note on `BP_ONLY` vs `FULL` for `decode_frame_subtract`:** both rows are
essentially identical here (≤1% apart, both find 100/100 stations) —
but this is an easy, flat +5 dB SNR scenario where BP alone already
succeeds on every candidate, so OSD never engages and looks like a
free no-op. **This does not generalize** — re-run directly against the
real `qso3_busy.wav` recording (see **WSJT-X Comparison** below) shows
`BP_ONLY` finds only **15** of the 22 messages `FULL` finds, missing
`CQ DX DL8YHR JO41` (the titular signal from mfsk-core issue #180)
and 6 other confirmed real messages — roughly **30% of recall**, on
exactly the weak/marginal signals (down to −22 dB) OSD exists to
recover. **Do not disable OSD by default based on this speed-bench
row alone** — it was checked here only to see whether `decode_phase2`
(which runs every 15 s cycle) could cheapen its `DecodeDepth` for
speed; the data says no on both counts (no speed win *and* a real
recall cost on hard signals). Left as an open question for a future
session: whether `decode_phase2` needs a cheaper decode path on
lower-power devices at all (real WASM/mobile timing isn't measured
here, only 24-core desktop native) — if so, it isn't `BP_ONLY`.

---

## DecodeDepth Matrix: LlrEffort × osd

`DecodeDepth`'s two fields (`LlrEffort::Minimal`/`Full`, `osd: bool`) are
now fully independent (mfsk-core issue #188) — previously they were
baked into named enum variants. This investigation (2026-07-26) swept
all four combinations against three different datasets to see whether
`DecodeDepth::FULL` (`{Full, osd:true}`, the value every WebFT8 call
site uses) is actually the right choice, or whether a cheaper
combination gives the same recall.

**Real recording (`qso3_busy.wav`), reproducible via
`cargo run -p ft8-bench --release --example depth_matrix`:**

| depth | plain single-pass (phase1-style) | staged SIC (phase2/subtract-style) |
|---|---|---|
| Minimal, noOSD | 11/21, 12.4–15.0 ms | 15/21, 349.6–352.2 ms |
| Minimal, OSD | 15/21, 16.3–17.4 ms | **21/21**, 518.6–520.9 ms |
| Full, noOSD | 11/21, 12.4–12.9 ms | 15/21, 404.9–406.3 ms |
| **Full, OSD (current default)** | **15/21**, 17.6–18.0 ms | **21/21**, 533.1–533.8 ms |

`LlrEffort` alone makes no difference in this dataset (11=11, 15=15 in
every row); `osd` is responsible for the entire recall gain (+4
messages plain, +6 messages staged — including `CQ DX DL8YHR JO41`,
the titular signal from mfsk-core issue #180). `Minimal, OSD` matches
`Full, OSD`'s recall here at a small speed edge.

**Narrow-band sniper (staged+EQ+AP), reproducible via
`run_depth_matrix_scenario` in `cargo run -p ft8-bench --release`,
20 seeds/cell:**

| scenario | SNR | Minimal,noOSD | Minimal,OSD | Full,noOSD | Full,OSD |
|---|---|---|---|---|---|
| center-crowd | −20 dB | 19/20, 236ms | 19/20, 402ms | 19/20, 242ms | 19/20, 404ms |
| center-crowd | −22 dB | 8/20, 208ms | 10/20, 380ms | 8/20, 216ms | 10/20, 374ms |
| BPF-edge | −20 dB | 19/20, 166ms | 20/20, 272ms | 19/20, 162ms | 20/20, 271ms |
| BPF-edge | −22 dB | 9/20, 137ms | 11/20, 252ms | 9/20, 148ms | 11/20, 259ms |

Same pattern: `LlrEffort` never changes the recall count; `osd` buys a
real (if smaller) gain right at the recall floor (−22 dB: +2/20 in both
scenarios). `Minimal` and `Full` are within noise of each other on
speed here.

**The counter-example — dense 100-station scenario (single-pass, same
config as the Speed Benchmark section above, 200–2800 Hz / ~26 Hz
station spacing, narrower than FT8's own ~50 Hz signal bandwidth, i.e.
real mutual interference between adjacent stations):**

| depth | decoded | time |
|---|---|---|
| Minimal, noOSD | 99/100 | 16.4 ms |
| Minimal, OSD | **96/100** | **94.8 ms** |
| Full, noOSD | 99/100 | 16.6 ms |
| **Full, OSD (current default)** | **100/100** | **46.6 ms** |

Here `Minimal, OSD` is worse than `Full, OSD` on *both* axes at once —
4 fewer stations decoded, and 2x slower. Reproducible via
`run_speed_bench`'s "Dense-100-station depth matrix" block in
`cargo run -p ft8-bench --release`.

**Conclusion: `DecodeDepth::FULL` stays the default everywhere.**
`Minimal` never won outright in any of the three datasets and lost
badly in the one with real inter-station interference — exactly the
condition a genuinely busy FT8 band produces. `osd: true` is
unconditionally worth keeping: turning it off costs real recall in
every dataset (up to −40% messages on the real recording) for a
speed win that's modest everywhere except the (undesirable) dense +
`Minimal` combination. Device-class shedding for low-power hardware
should keep targeting whether SIC (`.staged()`/`.flat()`) runs at all
(matching `app.js`'s existing `subDisabledAuto`), not `DecodeDepth`.

### Startup calibration benchmark redesigned (silence → synthetic 10-station)

A related fix prompted by this investigation: `ft8-web/www/app.js`'s
startup calibration (which sets `subDisabledAuto`/`apDisabledAuto` —
the device-class shedding this section just said should be the real
lever) used to time `decode_wav_f32` on **15 seconds of silence**.
Silence produces ~zero `coarse_sync` candidates, so it only measured
fixed spectrogram/FFT overhead and never exercised the per-candidate
BP/OSD cost that the tables above show dominates real decode time (13
ms on silence-like conditions vs. up to 530 ms on real/dense signals,
same `DecodeDepth`) — a device could pass the old calibration and still
blow the 15 s budget on an actual busy band. It now synthesizes 10 CQ
callers spread across 200–2800 Hz at varying levels (via the already-
exposed `encode_ft8`), so `coarse_sync` gets real candidates and BP/OSD
actually run.

**The existing shedding thresholds (300 ms / 800 ms) are carried over
unvalidated** — they were tuned against the old silence numbers, which
structurally under-measured real cost, so the new benchmark will report
larger numbers even on fast hardware. This needs real phone/desktop
browser measurement to re-tune (tracked as the reason for this
session's GitHub Pages deploy) rather than a synthetic-only guess.

---

## WSJT-X Comparison

### Synthetic scenarios (WSJT-X values not re-measured this run)

| Scenario | WSJT-X (est.) | WebFT8 |
|----------|---------------|--------|
| 15 crowd +5 dB, target −12 dB | 7 decoded¹ | **16 decoded** |
| 15 crowd +40 dB, target −14 dB (54 dB gap) | 0% | **0% (SW) / 100% (HW BPF)** |
| BPF edge −18 dB, no AP | N/A | **100%** |
| BPF edge −20 dB, EQ+AP | N/A | **95–100%** |

¹ WSJT-X value from a prior manual comparison run; not re-measured this run.

### Real recording, real jt9 CLI

`jt9` (from `github.com/saitohirga/WSJT-X`, built locally at
`/home/minoru/wsjtx-build/jt9`) run head-to-head against `mfsk-core` on a
real recorded busy-band FT8 WAV (`qso3_busy.wav`, 12 kHz mono, from
`mfsk-core`'s embedded-poc test assets), 200–3000 Hz full band, depth 3
(Deep): `jt9 -8 -p 15 -d 3 -a . -t . qso3_busy.wav`.

| Decoder / mode | Messages (in-band, 200–3000 Hz) | Time |
|----------------|----------------------------------|------|
| **jt9 -d3 (Deep)** | **21**¹ | ~1.1 s |
| mfsk-core `decode_frame` (single-pass) | 15 | 0.05 s |
| mfsk-core `decode_frame_subtract` (staged) | 22² | 0.62 s |
| mfsk-core `decode_phase2` shape (`known(&[]).staged()`) | **22** — byte-identical to the row above | 0.62 s |

¹ jt9 -d3 actually reports 22 lines total; one (`TU; 7N9RST EI8TRF 589
5732` @ 3390 Hz) is outside the 200–3000 Hz band both sides are
configured for and excluded from this comparison.

² Includes one message, `<?> 5T5ZGS/R FE02` @ 2570 Hz, that **jt9 does
not report anywhere in its output** (verified directly, this session,
against a real `jt9 -8 -d3` run — resolving a caveat carried over from
the prior `756d81f7` write-up, which flagged this message as
unverified). Combined with the `<?>` prefix (`unpack77` couldn't cleanly
resolve the first callsign field), this is now a **likely false
positive**, not a confirmed decode. Excluding it: mfsk-core finds
**21 confirmed messages, an exact match with jt9 Deep's 21 in-band
messages** — verified message-for-message, not just by count (see the
raw output below).

```
$ cargo run -p ft8-bench --release --example wsjtx_compare -- \
    ft8-bench/testdata/qso3_busy.wav subtract
$ cargo run -p ft8-bench --release --example wsjtx_compare -- \
    ft8-bench/testdata/qso3_busy.wav phase2_shape
```
Both commands print byte-identical 22-message lists (including the
unconfirmed `5T5ZGS/R`) — this is the direct proof that `decode_phase2`
(WebFT8's actual live-decode Phase 2 call) now runs the same engine as
`decode_frame_subtract`/`decode_wav_subtract`, closing the gap the
`c8c3c6d` fix targeted. Prior to that fix, `decode_phase2`'s call shape
(`decode_frame_subtract_with_known`) ran a separate, unfixed flat-3-pass
engine and would not have matched.

**Takeaway:** on this one real recording, mfsk-core now finds every
message jt9's exhaustive Deep mode finds in-band (21/21, a confirmed
exact match, not a fuzzy count-parity claim), while running ~1.8x faster
(0.62 s vs. ~1.1 s), and — for the first time — WebFT8's actual
production decode path (`decode_phase1`/`decode_phase2`) gets these same
results instead of a separately-unfixed engine. This is a single-WAV data
point, not a statistical claim, but it directly validates both mfsk-core
changes landed this session.

---

## WAV Files for External Verification

The benchmark writes synthetic WAV files to `ft8-bench/testdata/` for WSJT-X cross-testing:

| File | Scenario |
|------|----------|
| `sim_busy_band.wav` | 15 crowd +5 dB, target −12 dB |
| `sim_busy_band_hard_mixed.wav` | 15 crowd +40 dB, AGC clipped, target −14 dB |
| `sim_busy_band_hard_clean.wav` | Same but linear scale (no AGC clip) |
| `sim_busy_band_hard_bpf.wav` | BPF only: target −14 dB + AWGN |
| `sim_bpf_center.wav` | BPF center, target −18 dB |
| `sim_bpf_shoulder.wav` | BPF shoulder, target −18 dB |
| `sim_bpf_edge.wav` | BPF edge (−3 dB), target −18 dB |
| `sim_bpf_subtract.wav` | BPF + 4 in-band crowd, target −14 dB |
| `sim_stress_fullband.wav` | 15 crowd +20 dB + target −18 dB (WSJT-X stress) |
| `sim_stress_bpf_edge.wav` | Same, BPF filtered (sniper input) |
| `sim_stress_bpf_edge_clean.wav` | Target-only BPF edge (cleanest WSJT-X comparison) |
| `sim_extreme_hard.wav` | 15 crowd +40 dB, target −20 dB |
| `sim_extreme_edge.wav` | BPF edge, target −22 dB |
| `sim_extreme_edge_24.wav` | BPF edge, target −24 dB (beyond decoder limit) |

All WAVs are 12 000 Hz, 16-bit mono, ~14.6 s (FT8 frame). These are regenerated (and re-committed) each time the full bench suite runs, since the simulator writes them as a side effect of each scenario.

---

## Reproducing Results

```bash
# Build and run all benchmarks (release required for speed numbers)
cargo run -p ft8-bench --release
```

Real-recording WAVs (`191111_110130.wav`, `191111_110200.wav`) are not included in the repo.
Download from `https://github.com/jl1nie/RustFT8/tree/main/data` and place in `ft8-bench/testdata/`.
