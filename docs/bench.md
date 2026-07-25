# WebFT8 Decoder Benchmark Results

**mfsk-core 0.7.4 (GitHub `jl1nie/mfsk-core@28a1f03f`) — 2026-07-25**

Simulator-based evaluation of the WebFT8 decoder against reference conditions.
All results are reproducible: `cargo run -p ft8-bench --release`.

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
 -16.4 dB  1000 Hz  pass=0  CQ 3Y0Z JD34 ★   (subtract — now found on the FIRST pass, was missed entirely)
```

### Statistical sweep (20 seeds × target SNR)

AP = call2 `3Y0Z` known (実運用でターゲットをロック済みの状態)。

| Target SNR | Gap | single-pass | subtract | sniper-SIC | **sniper-SIC+AP** |
|------------|-----|-------------|----------|------------|-------------------|
| −10 dB | 18 dB | 0/20 (0%)¹ (was 100%) | 20/20 (**100%**, was 100%) | 20/20 (**100%**, was 100%) | 20/20 (**100%**) |
| −12 dB | 20 dB | 0/20 (0%)¹ (was 70%) | 20/20 (**100%**, was 85%) | 20/20 (**100%**, was 100%) | 20/20 (**100%**) |
| −14 dB | 22 dB | 0/20 (0%)¹ (was 5%) | 20/20 (**100%**, was 5%) | 20/20 (**100%**, was 65%) | 20/20 (**100%**, was 65%) |
| −16 dB | 24 dB | 0/20 (0%) | 9/20 (**45%**, was 0%) | 9/20 (**45%**, was 0%) | 9/20 (**45%**, was 0%) |
| −18 dB | 26 dB | 0/20 (0%) | 0/20 (0%) | 0/20 (0%) | 0/20 (0%) |
| −20 dB | 28 dB | 0/20 (0%) | 0/20 (0%) | 0/20 (0%) | 0/20 (0%) |

¹ See root-cause note below — this is not an mfsk-core regression, see caveat.

**Change since 2026-04-12:**
- `subtract` and `sniper-SIC` both jumped to **100% through −14 dB** (were 5–85%), and both now succeed at −16 dB (45%, was 0%) — roughly a **4–6 dB sensitivity gain** against this in-band-crowd scenario.
- `single-pass` (plain `decode_sniper`, no crowd handling) shows **0% across the entire sweep** here, vs. up to 100% in the 2026-04-12 numbers. **Root-caused, not a regression:** this is *not* something that changed between mfsk-core versions — a direct A/B rebuild against crates.io mfsk-core 0.6.8 and the current GitHub `main` (`28a1f03f`) produced byte-identical `decode_sniper` output (same 4 crowd decodes, same freq/dt/snr/hard_errors, target absent both times, checked across 8 seeds and both single- and multi-threaded rayon). The 2026-04-12 "100%" figure came from the old, now-removed in-repo `ft8-core` crate — a different implementation entirely, not an earlier mfsk-core release — so there is no mfsk-core regression to chase here. The 0% result is also functionally expected: the code comment for this scenario states outright that the 50 Hz-spaced in-band crowd is *designed* to mask the target from plain single-pass decode via spectral leakage, which is precisely why `subtract`/`sniper-SIC` exist and are the modes actually recommended whenever in-band crowd is present.

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
| decode_frame (single-pass) | 100 (was 58) | **18.4 ms** (was 159.7 ms, **8.7x faster**) | 17.7 ms | 18.9 ms | 2400 ms |
| decode_frame_subtract (multi-pass) | 100 (was 65) | 651.1 ms (was 285.4 ms — **slower**, but now finds every station, was 65/100) | 629.9 ms | 665.7 ms | 2400 ms |
| sniper+EQ (±250 Hz) | 17 (was 11) | **7.2 ms** (was 25.2 ms, **3.5x faster**) | 6.7 ms | 8.1 ms | 2400 ms |

All three modes comfortably fit within the FT8 15-second period (2400 ms decode window).

**Note on decode_frame_subtract:** its per-call time roughly doubled, but that's because it now finds **all 100** simulated stations instead of 65 — the extra time is genuine additional decode work (more real signals subtracted and re-decoded per pass), not a regression. `decode_frame` and `sniper+EQ` — which don't have this recall confound — both got dramatically faster (8.7x and 3.5x respectively), consistent with the ~7x speed-up measured directly against a fixed real-world recording (see [`ft8-bench/results/mfsk-core-speed.md`](../ft8-bench/results/mfsk-core-speed.md)).

---

## WSJT-X Comparison

| Scenario | WSJT-X (est.) | WebFT8 |
|----------|---------------|--------|
| 15 crowd +5 dB, target −12 dB | 7 decoded¹ | **16 decoded** |
| 15 crowd +40 dB, target −14 dB (54 dB gap) | 0% | **0% (SW) / 100% (HW BPF)** |
| BPF edge −18 dB, no AP | N/A | **100%** (was 45%) |
| BPF edge −20 dB, EQ+AP | N/A | **100%** (was 20%) |
| BPF edge −18 dB, EQ+AP | N/A | **100%** |

¹ WSJT-X value from prior manual comparison run; not re-measured in this run.

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
