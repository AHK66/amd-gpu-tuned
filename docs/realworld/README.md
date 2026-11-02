# Real-World Checks

Replayable, real-world verification of amd-gpu-tuned: the **real daemon**
against the **real GPUs**, driven through a **real browser** (Playwright MCP)
or a scripted battle test, in addition to the fixture-based e2e suite.

Purpose: catch unwanted side effects that unit/e2e fixtures cannot — real
telemetry tick rates, real card ranges, real sysfs round-trips, real load
times. Each entry is written so it can be re-run after a change to the code it
covers.

## Environment facts (agent-relevant)

- **sudo is NOPASSWD for exactly two commands** (rules live in
  `/etc/sudoers.d/` — see `sudo -l`), so an agent may start and stop the
  daemon itself without asking:
   ```bash
   # start/stop (from repo root)
   sh scripts/restart-amd-gpu-tuned.sh --no-build   # restart only
   sh scripts/restart-amd-gpu-tuned.sh              # rebuild + restart
   ```
   Caveats:
   - Never run `sudo pkill -f target/release/amd-gpu-tuned` inline — `-f`
     matches the calling shell's own command line and kills your session.
     Always go through `scripts/restart-amd-gpu-tuned.sh` (see `AGENTS.md`).
- Default listen address: `http://127.0.0.1:8787`.
- Port 8787 is **shared with the e2e suite** (Playwright webServer). Kill the
  manual daemon before `npm run e2e`, or e2e will reuse it and run against the
  real hardware/data dir instead of the fixtures.
- The binary must be current: `cargo build --release` first if Rust sources
  changed; the served UI is the embedded `ui/dist` bundle — `cd ui && npm run
  build` first if UI sources changed.
- Real data dir: the daemon writes to its default data location (presets,
  active state). Real-world checks that change GPU state (apply/revert) must
  restore the previous state (e.g. re-apply the prior preset or `stock`) and
  say so in their entry.

## How to run a check

1. Ensure the binary/bundle are fresh; start the daemon (above); confirm
   `ss -ltn | rg 8787` and `tail /tmp/amd-gpu-tuned.log` show GPUs enumerated.
2. Drive the UI via the Playwright MCP browser tools
   (`playwright_browser_navigate`, `playwright_browser_run_code_unsafe`, …)
   against `http://127.0.0.1:8787/`.
3. Record: exact steps, observed values, timings, pass/fail per assertion.
4. Restore any GPU state the check changed.
5. Write the entry below as `docs/realworld/YYYY-MM-DD-<topic>.md`.

## Entries

- [2026-10-12 — GpuTile preset clone/edit/delete: heal dangling source,
  blur-commit, label stays preset, multi-card delete guard (replayable
  procedure; Checks 1/2/3/5 superseded by the 2026-10-17 entry)](2026-10-12-gpu-tile-preset-clone-edit-delete.md)
- [2026-10-17 — GpuTile transient preset edits: modified tracking, Save
  (merge), switch guard (Save/Stay/Dismiss), leave-Dashboard guard,
  Presets-panel in-use delete guard; tile trash removed (replayable
  procedure)](2026-10-17-transient-preset-edits.md)
- [2026-10-17 — Presets panel refinement: trimmed toolbar (no
  revert/baseline), no knobs column, row actions always visible in a
  `hover:none` browser (the `@media (hover:hover)` gating fix); in-use delete
  guard + export re-confirmed (replayable procedure)](2026-10-17-presets-panel-refinement.md)
- [2026-10-24 — Auth battle test: live-daemon password-gate suite, scripted via
  `scripts/auth-battle-test.sh`](2026-10-24-auth-battle-test.md)
