# WebFT8 Decoder Benchmark Results

**mfsk-core 0.7.4 (GitHub `jl1nie/mfsk-core@7bc1684a`) — 2026-07-25**

Simulator-based evaluation of the WebFT8 decoder against reference conditions.
All results are reproducible: `cargo run -p ft8-bench --release`.

> **Updated same-day from the `28a1f03f` run below** (mfsk-core issues
> [#177](https://github.com/jl1nie/mfsk-core/issues/177)/[#179](https://github.com/jl1nie/mfsk-core/issues/179)/[#180](https://github.com/jl1nie/mfsk-core/issues/180),
> merged [jl1nie/mfsk-core#178](https://github.com/jl1nie/mfsk-core/pull/178)).
> Root cause: `subtract_tones_lpf`'s SIC low-pass kernel had its cosine²
> window argument divided by `lpf_half` instead of `NFILT` (`=
> 2*lpf_half`), doubling the taper's argument range — instead of a
> smooth taper to 0 at the window edges, the kernel gave samples 166 ms
> away **full weight**, same as the current sample, actively corrupting
> the QSB/channel estimate instead of smoothing it. This bug had been
> present since `subtract_tones_lpf` became the canonical FT8/FT4 SIC
> path (mfsk-core v0.6.2) — i.e. every prior "since 2026-04-12" number
> below that involves `subtract`/`sniper-SIC` was itself already
> running on a broken SIC low-pass; this update is the first time that
> path has been numerically correct. **Only scenarios that exercise
> in-band-crowd SIC moved** (Scenario 5, and the real-recording jt9
> comparison below) — every other scenario (1–4, 6–8, FT4 sweep,
> speed benchmark) reproduced byte-identical to the `28a1f03f` numbers,
> confirming the fix's effect is correctly scoped. Deltas vs.
> `28a1f03f` are called out inline where they exist.
>
> **Updated from the 2026-04-12 (ft8-core v0.3.0) run.** The decode engine
> moved from the in-repo `ft8-core` crate to the external
> [`mfsk-core`](https://github.com/jl1nie/mfsk-core) crate, which brought
> both a ~7x decode speed-up (see
> [`ft8-bench/results/mfsk-core-speed.md`](../ft8-bench/results/mfsk-core-speed.md))
> and — as this re-run shows — a substantial accuracy/sensitivity
> improvement. Deltas vs. the 2026-04-12 numbers are called out inline
> below. **Caveat:** the 2026-04-12 baseline was produced by that old,
> now-removed in-repo `ft8-core` crate, not an earlier mfsk-core version —
> the "since 2026-04-12" deltas below reflect a full decode-engine swap,
> not incremental tuning of the same engine. Root-caused two specific
> mfsk-core commits between the crates.io 0.6.7/0.6.8 releases and the
> current GitHub `main` (`28a1f03f`) that explain the accuracy gains
> within mfsk-core's own history: `OSD_HARDERRORS_MAX` widened 22→36
> (WSJT-X-faithful, mfsk-core#152/issue #72) and rayon-parallelised
> `coarse_sync` (mfsk-core#139) — see the Scenario 5 note below for how
> this was verified. The ft8-bench scenario seed-loops are now
> parallelized with `rayon`, so re-running the full suite takes ~35 s
> instead of ~2m15s on a 24-core box (identical results either way —
> verified by diffing serial vs. parallel output).

---

## Test Environment

| Item | Value |
|------|-------|
| Decoder | mfsk-core 0.7.4, git `28a1f03f` (Rust, native release) |
| Signal model | Pure-tone 8-GFSK + AWGN (12 000 Hz, i16) |
| BPF model | 4-pole Butterworth, 500 Hz passband |
| Seed count | 20–30 independent noise realisations per cell |
| Platform | x86-64 Linux (WSL2), 24-core, Rayon thread-pool (nested: ft8-bench seed-loops + mfsk-core's own internal candidate parallelism) |

### Decoder Modes

| Mode | Description |
|------|-------------|
| `full-band` | `decode_frame` — 200–2800 Hz, equivalent to WSJT-X |
| `subtract` | `decode_frame_subtract` — multi-pass subtract + QSB gate |
| `sniper` | `decode_sniper` — ±250 Hz around target freq |
| `sniper+EQ` | `decode_sniper_eq(Adaptive)` — sniper + Costas Wiener EQ |
| `sniper+AP` | `decode_sniper_ap` — sniper + EQ + A Priori callsign lock |
| `sniper-SIC` | `decode_sniper_sic` — sniper + EQ + in-band SIC |

---

## Scenario 1 — Single +40 dB Interferer (200 Hz offset)

Target: `CQ 3Y0Z JD34` @ 1000 Hz, SNR −5 dB
Interferer: `CQ JQ1QSO PM95` @ 1200 Hz, SNR +35 dB
Seed: 99

| Mode | Target | Interferer | Total decoded |
|------|--------|-----------|---------------|
| full-band | **DECODED** ⬆ (was missed) | DECODED | 2 (was 1) |
| sniper (BPF removes interferer) | **DECODED** | — | 1 |

**Change since 2026-04-12:** previously a single +40 dB station 200 Hz away completely masked the target in full-band mode — only the hardware-BPF sniper path recovered it. With the current decoder, **full-band decode now recovers the target too**, without any physical filtering. The sniper path is no longer the only way to decode this scenario, though it remains useful for heavier crowds (see Scenario 3).

---

## Scenario 2 — Busy Band, Moderate Crowd

15 crowd stations @ **+5 dB**, target `CQ 3Y0Z JD34` @ 1000 Hz, **−12 dB**, seed 777

| Mode | Target | Total decoded |
|------|--------|---------------|
| full-band | **DECODED** | 16 / 16 |
| sniper | **DECODED** | 2 |

No material change from 2026-04-12 — this scenario was already at ceiling.

---

## Scenario 3 — Busy Band, Hard ADC Saturation

15 crowd stations @ **+40 dB**, target @ **−14 dB** (gap = 54 dB), seed 888

The AGC of a 16-bit ADC scales for the crowd; the −14 dB target occupies only a few LSBs.

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

Unchanged from 2026-04-12: no software-only technique recovers the target when a 54 dB crowd fully occupies the ADC dynamic range — this remains the core justification for the hardware BPF. For the "with hardware BPF" recovery rate at this same crowd level, see **Scenario 8**, which runs the identical +40 dB / 500 Hz-BPF setup across a −10…−22 dB target sweep (100% through −18 dB, was 0–100% depending on filter/SNR in the 2026-04-12 run).

---

## Scenario 4 — BPF Edge Distortion

Target only + AWGN, SNR **−18 dB**, 20 seeds, 4-pole Butterworth 500 Hz BPF.
Three placements relative to the passband centre.

| Placement | BPF window (Hz) | Target attenuation | EQ OFF | EQ ON |
|-----------|-----------------|--------------------|--------|-------|
| center | 750 – 1250 | −0.0 dB | 20/20 (**100%**, was 60%) | 20/20 (**100%**, was 70%) |
| shoulder | 950 – 1450 | −0.5 dB | 20/20 (**100%**, was 30%) | 20/20 (**100%**, was 50%) |
| edge (−3 dB) | 1000 – 1500 | −3.0 dB | 20/20 (**100%**, was 20%) | 20/20 (**100%**, was 45%) |
| no-BPF (reference) | — | 0 dB | 20/20 (**100%**, was 40%) | — |

Filter response (centre = 1000 Hz, 4-pole Butterworth) — unchanged, filter design didn't change:

| Freq (Hz) | Attenuation |
|-----------|-------------|
| 750 | −3.0 dB |
| 800 | −0.4 dB |
| 900 | −0.0 dB |
| 1000 | −0.0 dB |
| 1200 | −0.9 dB |
| 1250 | −3.0 dB |
| 1300 | −6.4 dB |
| 1500 | −20.2 dB |

**Change since 2026-04-12:** every cell in this scenario saturated at 100%, up from 20–70%. At −18 dB target SNR, the current decoder no longer needs the equalizer to close the gap here — even EQ OFF is at 100% across every filter placement including the −3 dB edge. This is one of the clearest signs of a genuine sensitivity gain in mfsk-core's core LLR/BP-OSD path, not just an EQ improvement.

---

## Scenario 5 — BPF + In-Band Crowd (Signal Subtraction)

4 crowd stations **inside** the 500 Hz passband (850, 950, 1050, 1150 Hz), SNR **+8 dB** (fixed).
Target `CQ 3Y0Z JD34` @ 1000 Hz.
BPF: 750–1250 Hz, 4-pole Butterworth.

### Example decode (target −14 dB, seed 1234)

| Mode | Target | Total decoded |
|------|--------|---------------|
| single-pass sniper | **missed** | 4 (crowd only) |
| subtract (full-band) | **DECODED** ⬆ (was missed) | 5 |
| sniper-SIC | **DECODED** | 5 |

```
  +2.6 dB   950 Hz  pass=0  CQ JQ1QRM PM95
  +2.7 dB  1050 Hz  pass=0  CQ JQ1QRN PM96
  +2.8 dB   850 Hz  pass=0  CQ JQ1QSO PM95
  +2.9 dB  1150 Hz  pass=0  CQ JQ1QRP PM85
 -15.7 dB  1000 Hz  pass=0  CQ 3Y0Z JD34 ★   (subtract, 7bc1684a — was -16.4 dB @ 28a1f03f;
                                               small shift because the LPF fix changed the
                                               post-subtract residual the SNR is estimated from)
```

### Statistical sweep (20 seeds × target SNR)

AP = call2 `3Y0Z` known (実運用でターゲットをロック済みの状態)。

| Target SNR | Gap | single-pass | subtract | sniper-SIC | **sniper-SIC+AP** |
|------------|-----|-------------|----------|------------|-------------------|
| −10 dB | 18 dB | 0/20 (0%)¹ | 20/20 (**100%**) | 20/20 (**100%**) | 20/20 (**100%**) |
| −12 dB | 20 dB | 0/20 (0%)¹ | 20/20 (**100%**) | 20/20 (**100%**) | 20/20 (**100%**) |
| −14 dB | 22 dB | 0/20 (0%)¹ | 20/20 (**100%**) | 20/20 (**100%**) | 20/20 (**100%**) |
| −16 dB | 24 dB | 0/20 (0%) | 20/20 (**100%**, was 45%) | 20/20 (**100%**, was 45%) | 20/20 (**100%**, was 45%) |
| −18 dB | 26 dB | 0/20 (0%) | 20/20 (**100%**, was 0%) | 20/20 (**100%**, was 0%) | 20/20 (**100%**, was 0%) |
| −20 dB | 28 dB | 0/20 (0%) | 2/20 (**10%**, was 0%) | 7/20 (**35%**, was 0%) | 13/20 (**65%**, was 0%) |

("was …" in this table = the `28a1f03f` run earlier the same day, i.e. the SIC-LPF-kernel-bug fix's effect — see the header note. Rows/columns with no "was" annotation matched `28a1f03f` exactly.)

¹ See root-cause note below — this is not an mfsk-core regression, see caveat.

**Change since `28a1f03f` (this update, same-day — SIC LPF kernel fix, mfsk-core#180):** the 100% floor extended from −14 dB to **−18 dB** (a 4 dB extension), and −20 dB — previously a total 0% wipeout for every SIC-based mode — now recovers **10–65%** of frames depending on mode. This is the direct, isolated effect of fixing `subtract_tones_lpf`'s LPF window (see header note): every one of these rows exercises signal subtraction against in-band interference, exactly the code path the bug lived in.

**Change since 2026-04-12 (decode-engine swap, `ft8-core`→`mfsk-core`):**
- `subtract` and `sniper-SIC` both jumped to **100% through −18 dB** (were 5–85% through only −14 dB in the old engine), and now partially succeed at −20 dB (10–65%, was 0%) — a substantial sensitivity gain against this in-band-crowd scenario, further extended by the same-day SIC LPF fix above.
- `single-pass` (plain `decode_sniper`, no crowd handling) shows **0% across the entire sweep** here, vs. up to 100% in the 2026-04-12 numbers. **Root-caused, not a regression:** this is *not* something that changed between mfsk-core versions — a direct A/B rebuild against crates.io mfsk-core 0.6.8 and the current GitHub `main` produced byte-identical `decode_sniper` output (same 4 crowd decodes, same freq/dt/snr/hard_errors, target absent both times, checked across 8 seeds and both single- and multi-threaded rayon). The 2026-04-12 "100%" figure came from the old, now-removed in-repo `ft8-core` crate — a different implementation entirely, not an earlier mfsk-core release — so there is no mfsk-core regression to chase here. The 0% result is also functionally expected: the code comment for this scenario states outright that the 50 Hz-spaced in-band crowd is *designed* to mask the target from plain single-pass decode via spectral leakage, which is precisely why `subtract`/`sniper-SIC` exist and are the modes actually recommended whenever in-band crowd is present.

---

## Scenario 6 — SNR Sensitivity: BPF Edge

BPF edge placement (target at −3 dB point), 20 seeds per row.
Target: `CQ 3Y0Z JD34`, target + AWGN only.

| SNR | EQ OFF | EQ | EQ + AP (CQ+call2) | full AP (77-bit) |
|-----|--------|----|--------------------|-------------------|
| −18 dB | 20/20 (**100%**, was 20%) | 20/20 (**100%**, was 45%) | 20/20 (**100%**, was 100%) | 20/20 (100%) |
| −20 dB | 18/20 (**90%**, was 0%) | 15/20 (**75%**, was 0%) | 20/20 (**100%**, was 20%) | 19/20 (95%) |
| −22 dB | 1/20 (5%, was 0%) | 3/20 (15%, was 0%) | 10/20 (**50%**, was 0%) | 8/20 (40%) |
| −24 dB | 0/20 (0%) | 0/20 (0%) | 0/20 (0%) | 0/20 (0%) |

AP = A Priori decoding with known callsign `3Y0Z` as call2 (61-bit lock, pass 7); full AP additionally locks a simulated own-callsign for the 77-bit passes.

**Change since 2026-04-12:** the practical decode floor moved by roughly **2 dB**. EQ+AP used to hit 100% only at −18 dB and collapse to 20% one step down; now it holds 100% at −20 dB and still recovers half the frames at −22 dB. EQ OFF alone — no equalizer, no AP — now matches the *old* EQ+AP number at −18 dB (100%).

---

## Scenario 7 — Full QSO: BPF Edge + AP

All QSO message types across a simulated `JA1ABC ↔ 3Y0Z` exchange.
BPF edge (1000–1500 Hz, 4-pole), 20 seeds each.

| SNR | CQ (61-bit) | REPORT (61-bit) | RR73 (77-bit) |
|-----|-------------|-----------------|---------------|
| −18 dB | 20/20 (**100%**, was 95%) | 20/20 (**100%**, was 90%) | 20/20 (100%) |
| −20 dB | 20/20 (**100%**, was 40%) | 20/20 (**100%**, was 40%) | 20/20 (**100%**, was 75%) |
| −22 dB | 10/20 (**50%**, was 10%) | 10/20 (**50%**, was 5%) | 15/20 (**75%**, was 20%) |
| −24 dB | 0/20 (0%, 1 false positive) | 1/20 (5%) | 3/20 (15%) |

**Change since 2026-04-12:** −20 dB went from a 40–75% success band to a clean **100% across every message type** — the single biggest jump in this scenario. −22 dB roughly quintupled (10→50%, 5→50%, 20→75%).

---

## Scenario 8 — Filter Comparison: Butterworth vs Elliptic (4-pole, 500 Hz BW)

15 crowd @ +40 dB (hardware BPF removes crowd before ADC; target + AWGN only after BPF).
EQ + AP (call2 = `3Y0Z`) applied to all BPF columns. 20 seeds per cell.

| SNR | no-BPF | BW-edge+EQ+AP | BW-center+EQ+AP | EL-edge+EQ+AP | EL-center+EQ+AP |
|-----|--------|---------------|-----------------|---------------|-----------------|
| −10 dB | 0% | **100%** | **100%** | **100%** | **100%** |
| −12 dB | 0% | **100%** | **100%** | **100%** | **100%** |
| −14 dB | 0% | **100%** | **100%** | **100%** | **100%** |
| −16 dB | 0% | **100%** | **100%** | **100%** | **100%** |
| −18 dB | 0% | **100%** | **100%** (was 95%) | **100%** (was 95%) | **100%** (was 95%) |
| −20 dB | 0% | **95%** (was 20%) | **100%** (was 40%) | **95%** (was 20%) | **100%** (was 35%) |
| −22 dB | 0% | **45%** (was 0%) | **50%** (was 10%) | **40%** (was 0%) | **50%** (was 10%) |

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

**Change since 2026-04-12 — the headline number:** at **−20 dB**, success rates roughly **tripled to quintupled** (BW-edge 20%→95%, BW-center 40%→100%, EL-edge 20%→95%, EL-center 35%→100%). At −22 dB, a scenario that was previously a near-total wipeout (0–10%) now recovers **40–50%** of frames. Butterworth-vs-Elliptic and edge-vs-center relative ordering is unchanged (Butterworth still slightly better at the edge, negligible difference at centre) — this is a decoder sensitivity gain, not a filter-choice effect.

Raw data (pre-upgrade, 2026-04-12): [`ft8-bench/results/elliptic_vs_butterworth_4pole.txt`](../ft8-bench/results/elliptic_vs_butterworth_4pole.txt) — kept for historical reference; the table above supersedes it.

---

## Speed Benchmark

100 stations, 200–2800 Hz, SNR +5 dB, 10 runs after 3 warmup.
Release build (`cargo run -p ft8-bench --release`), Linux x86-64 (24-core).

| Mode | Decoded | Mean | Min | Max | Budget |
|------|---------|------|-----|-----|--------|
| decode_frame (single-pass) | 100 (was 58) | **19.3 ms** (was 159.7 ms pre-mfsk-core, **8.2x faster**) | 18.2 ms | 20.9 ms | 2400 ms |
| decode_frame_subtract (multi-pass) | 100 (was 65) | 615.6 ms (was 285.4 ms pre-mfsk-core — **slower**, but now finds every station, was 65/100) | 593.0 ms | 634.8 ms | 2400 ms |
| sniper+EQ (±250 Hz) | 17 (was 11) | **7.4 ms** (was 25.2 ms pre-mfsk-core, **3.4x faster**) | 6.9 ms | 7.8 ms | 2400 ms |

All three modes comfortably fit within the FT8 15-second period (2400 ms decode window).

**Note on decode_frame_subtract:** its per-call time roughly doubled vs. the pre-mfsk-core baseline, but that's because it now finds **all 100** simulated stations instead of 65 — the extra time is genuine additional decode work (more real signals subtracted and re-decoded per pass), not a regression. `decode_frame` and `sniper+EQ` — which don't have this recall confound — both got dramatically faster (8.2x and 3.4x respectively), consistent with the ~7x speed-up measured directly against a fixed real-world recording (see [`ft8-bench/results/mfsk-core-speed.md`](../ft8-bench/results/mfsk-core-speed.md)).

**Re-verified `7bc1684a` (this update's SIC LPF kernel fix):** all three numbers above reproduce within ~5% run-to-run noise of the `28a1f03f` figures on this 100-station synthetic scenario (already at 100/100 decode ceiling for `decode_frame`/`decode_frame_subtract` pre-fix, so there's no headroom left for the fix to show up here) — no speed regression from the fix on this workload. The fix's cost is concentrated in `decode_frame_subtract_staged`'s dt-refinement search (not exercised by any of the three modes above); see mfsk-core issue #180 for that path's own ~1.3–1.9x-of-flat-pass cost measurement.

---

## WSJT-X Comparison

### Synthetic scenarios (WSJT-X values not re-measured this run)

| Scenario | WSJT-X (est.) | WebFT8 |
|----------|---------------|--------|
| 15 crowd +5 dB, target −12 dB | 7 decoded¹ | **16 decoded** |
| 15 crowd +40 dB, target −14 dB (54 dB gap) | 0% | **0% (SW) / 100% (HW BPF)** |
| BPF edge −18 dB, no AP | N/A | **100%** (was 45%) |
| BPF edge −20 dB, EQ+AP | N/A | **100%** (was 20%) |
| BPF edge −18 dB, EQ+AP | N/A | **100%** |

¹ WSJT-X value from prior manual comparison run; not re-measured in this run.

### Real recording, real jt9 CLI — 2026-07-25

The synthetic scenarios above never had a live WSJT-X binary run against
them. To close that gap, WSJT-X's `jt9` command-line decoder (from
`github.com/saitohirga/WSJT-X`, built locally — `cmake --build . --target
jt9`) was run head-to-head against `mfsk-core` on a real recorded busy-band
FT8 WAV (`qso3_busy.wav`, 12 kHz mono, from `mfsk-core`'s embedded-poc test
assets), 200–3000 Hz, full band. `jt9 -d` sets WSJT-X's own decode depth
(1=Fast, 2=Normal, 3=Deep); `mfsk-core`'s `decode_frame` (single-pass) and
`decode_frame_subtract` (multi-pass) are the closest equivalents.

| Decoder / mode | Messages | Time | vs. jt9 -d3 (Deep) |
|----------------|----------|------|---------------------|
| jt9 -d1 (Fast) | 14 | 0.23 s | — |
| jt9 -d2 (Normal) | 19 | 0.56 s | — |
| **jt9 -d3 (Deep)** | **22** | 1.11 s | baseline |
| mfsk-core `decode_frame` (single-pass) | 14¹ | **0.02 s** | 64% recall, **11x faster** than jt9 -d1 |
| mfsk-core `decode_frame_subtract` (multi-pass) | 19 (was 18) | **0.32 s** | **90% recall**² (19/21 in-band, was 86%), **3.5x faster** than jt9 -d3 |

¹ Corrects a stale "13" in the prior `28a1f03f` write-up of this table —
re-verified directly against both `28a1f03f` and `7bc1684a` (via a local
`[patch]` override), byte-identical 14-message output both times.
`decode_frame` never calls the SIC subtract path this update's fix
touched, so — unlike the `decode_frame_subtract` row — this number was
never expected to move; it just hadn't been checked as carefully before.

² Excludes one jt9 -d3 decode at 3390 Hz, outside the 200–3000 Hz FT8 band
both sides were configured for — not a fair miss to count either way.

Message-level diff (normalizing jt9's zero-padded SNR format, e.g. `-09` vs
`-9`) confirms **every message `decode_frame_subtract` reported was also
independently found by jt9 — zero false positives**. The only misses now
are 2 real decodes exclusive to jt9's exhaustive Deep mode (`CQ DX DL8YHR
JO41`, `K1BZM DK8NE -10`) — down from 3 as of the `28a1f03f` run, which
also missed `WA2FZW DL5AXX RR73` (2546 Hz): that signal is now decoded,
the direct, isolated effect of this update's SIC-LPF-kernel fix (see
header note) — the exact same 13th early-subtracted interferer that
motivated mfsk-core issue #180 in the first place. `DL8YHR` itself (the
issue's own titular signal, ~-17 dB) is decoded by mfsk-core's newer
*staged-checkpoint* SIC (`decode_frame_subtract_staged`, mfsk-core
0.8.0+) — not yet wired into `decode_frame_subtract` or any WebFT8
production caller, so it doesn't show up in this particular table; see
mfsk-core issue #180 for that path's own results. Notably, `mfsk-core`
still recovers all three historical "CRC-luck phantom" candidates its own
`OSD_HARDERRORS_MAX` gate used to filter (`N1API F2VX 73`, `N1API HA6FQ
-23`, `CQ EA2BFM IN83`) — and jt9 independently confirms all three as real
messages, which is strong external evidence they were never phantoms.

**Takeaway:** on this one real recording, `decode_frame_subtract` now lands
right at the edge of jt9's Deep decode depth — 19 decodes vs. 19 (Normal)
and 22 (Deep), i.e. it now *matches* jt9 Normal's count while running
1.75x faster than Normal and 3.5x faster than Deep, and is only 3 messages
behind Deep mode (was 4). This is a single-WAV data point, not a
statistical claim, but it's the clearest concrete evidence yet that the
mfsk-core#180 SIC fix (see header note) is a real accuracy gain on genuine
recorded traffic, not just synthetic-scenario tuning.

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
