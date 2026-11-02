# R9700 notes: mclk stability limit and undervolt/cap interplay — 2026-10-12

Working notes on two questions: why inference collapses above ~1400 MHz mclk
despite cool VRAM, and how the undervolt interacts with the power cap.

## 1. Memory clock: why the collapse is invisible to thermals

- **Memory stability is a timing-margin limit, not a thermal one.** Raising
  mclk shrinks the DRAM timing window (tCL, tRFC, … are fixed). Past the
  margin limit, individual cells/bits become marginal. This is
  electrical/signal-integrity, so VRAM temp sensors read normal.
- **The R9700 has UMC ECC** (RAS sysfs `umc_err_count`, verified on both
  cards):
  - single-bit flips per codeword → corrected, counted in `ce`;
  - 2+ bits in one codeword → uncorrectable (`ue`), and *that* is the data
    that corrupts weights/KV cache and collapses LLM inference;
  - `de` climbs as the controller retires bad cells.
- **LLM inference is a brutal VRAM stressor**: a 27B model at TP2 reads the
  full VRAM at full bandwidth continuously for minutes/hours, hitting all
  banks/channels — far harsher than a 30 s settle window or a game.
  Marginal bits fail *probabilistically* → intermittent collapse, no crash.
- **Diagnostic gap**: if collapses happen with flat CE/UE, the cause is not
  the cell array — look at sclk/VDDGFX margin, power delivery, or the
  driver's mclk retrain/PHY path (a marginal VDDM rail can glitch without
  the DRAM logging an ECC error).

### Suggested experiment (ECC tile is half-built)

1. Fixed inference loop at stock mclk → record CE/UE delta (baseline).
2. Repeat at 1400 / 1425 / 1450 MHz.
3. CE rate climbs steeply past ~1400 → confirmed memory margin limit; back
   off to the last clean step, or nudge VDDM up a few mV for 20–50 MHz
   headroom.
4. Collapses with flat CE/UE → memory exonerated; investigate the
   compute/rail side.

Context: stock mclk ≈ 1350 MHz (2700 MT/s GDDR6). An external reference
(another dual-R9700 box) found 2700 MHz helped decode +5.3 % vs prefill
+0.1–0.6 %. Observed stable ceiling on this box: **~1400 MHz**, collapses
beyond.

## 2. Undervolt × power cap: the interplay

- **The power cap is the binding constraint, not the voltage.** Most of the
  performance floor comes from the cap alone; the undervolt buys back clock
  *within* that cap (P ≈ C·V²·f: lower V → cheaper cycles → higher
  sustainable sclk at the same watt budget).
- **sweet80 (220 W + −80 mV)** ≈ stock performance: −4 % prefill, ~0 %
  decode (memory-bound, cap-immune), −14 °C prefill / −5 °C decode on the
  hot card, −23 W average. Heat scales with power, so the thermal win is
  nearly free.
- **−70 mV at the 300 W cap** is a throughput play, not a thermal one:
  the card pushes sclk *higher* until the 300 W wall (≈ +2…+5 % prefill,
  decode flat), at stock ~96–99 °C hotspot peaks. Stability margin shrinks
  because it moves *up* the V/f curve on a *lower* voltage — failure mode
  is random hangs under peak load. Only worth it if prefill throughput
  matters more than thermals; validate with long 32K/64K prefill runs.

## 3. Rule of thumb: iso-performance cap

At equal performance, power scales with V²:

**W_needed ≈ W_stock × (V_new / V_stock)²**

With a ~1.0 V nominal rail, **each −20 mV ≈ −4 % power** for equal
performance.

| Undervolt | (V ratio)² | 300 W equivalent cap | Observed cap |
|-----------|-----------|---------------------|--------------|
| −20 mV    | ≈ 0.96    | ~288 W              | 280 W ✓      |
| −75 mV    | ≈ 0.86    | ~256 W              | 220 W (below → ~−4 % prefill) ✓ |

Usage:

- Cap **at** the iso-performance number → free cooling, ~0 % loss.
- Cap **below** it → trading throughput (220 W case).
- Cap **above** it → no benefit, just unneeded voltage headroom.

Caveats:

1. First-order approximation (±5 %): the V/f curve isn't perfectly
   parabolic and driver power management adds inefficiency near the cap.
2. This gives the **performance-neutral** point, not the **stability**
   limit. A lower cap than the iso point buys a wider stability margin —
   why −80 mV@220 W was chosen over −75 mV@~256 W on this box.
3. Per-card variation is real (external reference: one card took −100 mV,
   the other was questionable past −80 mV). Treat the formula as a starting
   point; validate with the trial pipeline.
