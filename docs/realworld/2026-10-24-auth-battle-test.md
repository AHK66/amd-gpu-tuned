# Auth Battle Test — Live-Daemon Password-Gate Suite

Date: 2026-10-24
Repo HEAD at run: df3bd11

## Purpose

The battle test proves the write-endpoint password feature end-to-end against
the **live** daemon on the real machine: `set-password` taking effect on a
running daemon (no restart), a global `lock` killing **all** tokens, a
restart invalidating tokens while the argon2id hash persists, live password
change and `--clear`, and the UI unlock-modal flow. It drives the API
scenarios with `curl`, the UI scenarios with Playwright against the same
daemon, then cleans up — and stays re-runnable, always ending in open mode.

## Prerequisites

- daemon running: `sh scripts/restart-amd-gpu-tuned.sh`
- box in **open mode** (`curl -s http://127.0.0.1:8787/api/status` shows `auth_enabled: false`)
- Playwright chromium: `cd ui && npx playwright install chromium`

## How to run

`sh scripts/auth-battle-test.sh` — ~3 min. Ends in open mode.
Mid-run kill recovery (from the repo root): `sudo -n target/release/amd-gpu-tuned set-password --clear`.

## Scenarios

### API + UI phases (runner, `scripts/auth-battle-test.sh`)

| # | Scenario | Actions | Expected behavior (pass criteria) |
|---|----------|---------|-----------------------------------|
| 0 | Preflight | daemon reachable; `auth_enabled` is `false` (else abort); ungated `probe` | probe → 200 |
| 1 | Live set-password | `set_pw $PW1` (no restart) | `auth_enabled` `true`; ungated `probe` → 401 with body exactly `{"error":"authentication required"}`; all seven reads (`status`, `telemetry`, `presets`, `drift`, `events`, `setup`, `exporter/metrics`) → 200; `unlock $PW1` → 200, `token` matches `^[0-9a-f]{64}$`, `ttl_s` = 1800 |
| 2 | Wrong password | `unlock` with `wrong-password-1` | 401; `GET /api/events` contains an `auth_unlock_failed` entry |
| 3 | Token gates writes | `probe` with token → then ungated | 200 then 401; ring minutes unchanged after the 401 |
| 4 | Concurrent unlocks | `unlock $PW1` twice → tokens A, B | A ≠ B; `probe` succeeds with each |
| 5 | Global lock | `POST /api/auth/lock` with A; then `probe` with A and B; then ungated `lock` | `{"locked":true}`; both probes → 401; ungated `lock` → 401 |
| 6 | Live password change | `set_pw $PW2`; `unlock $PW1`; `unlock $PW2` | PW1 → 401; PW2 → 200 |
| 7 | Restart invalidates tokens | `unlock $PW2` → T; `sh scripts/restart-amd-gpu-tuned.sh --no-build`; `wait_daemon 60`; `probe` with T; `unlock $PW2` | probe with T → 401; `auth_enabled` still `true` (hash survived); fresh unlock → 200 |
| B | UI locked mode | Playwright `e2e/auth.spec.ts -g "locked mode"` | all locked-mode tests pass |
| 8 | Live --clear | `set_pw ''` | `auth_enabled` `false`; ungated `probe` → 200; `unlock $PW2` → 400 |
| D | UI open mode | Playwright `e2e/auth.spec.ts -g "open mode"` | open-mode test passes |
| 9 | CLI validation | pty pipe `short`/`short`; pty pipe `battle-test-pw-3`/`battle-test-pw-4` (mismatch) | both exit non-zero (min length / mismatch); `auth_enabled` still `false` and `unlock` → 400 (no hash written) |
| F | Cleanup assert | if still locked, `set_pw ''`; assert open | `auth_enabled` `false`, ungated `probe` → 200 |

### UI flows (Playwright, `ui/e2e/auth.spec.ts`)

| Mode | Test | Expected behavior (pass criteria) |
|------|------|-----------------------------------|
| locked | gated write opens the unlock dialog | `commitRing` → dialog titled "Unlock writes" visible with `unlock-password` |
| locked | wrong password shows the error | trigger → fill `wrong-password-1` → submit → `unlock-error` shows "wrong password"; dialog stays open |
| locked | correct password unlocks and the write lands | trigger → fill `PW` → submit → `commitRing` again → dialog hidden; second commit raises no dialog; `lock-indicator` reads "unlocked" |
| locked | lock now | unlock; click `lock-indicator` → confirm "lock now" → `lock-indicator` reads "locked"; next `commitRing` re-opens the dialog |
| locked | page reload drops the token | unlock; `page.reload()` → `lock-indicator` reads "locked" (token was in-memory only) |
| locked | no token in page storage | unlock; inspect `localStorage` + `sessionStorage` → no key or value matches a bearer token (`[0-9a-f]{64}`) |
| open | setup shows the no-password note | `page.goto("/#/setup")` → `no-password-note` visible; `lock-indicator` absent |

## Last run

2026-10-24 on daemon df3bd11 — **22/22 passed**, final state open mode.

| # | Scenario | Result | Key evidence |
|---|----------|--------|--------------|
| 0 | preflight | PASS | daemon up, open mode, ungated probe 200 |
| 1 | live set-password | PASS | 401 body `{"error":"authentication required"}`; token 64-hex, ttl_s 1800 |
| 2 | wrong password | PASS | 401 + `auth_unlock_failed` in /api/events |
| 3 | token gates writes | PASS | 200 with token, 401 without, ring unchanged (15) |
| 4 | concurrent unlocks | PASS | A ≠ B, both gate |
| 5 | global lock | PASS | lock 200 `{locked:true}`, A+B dead, ungated lock 401 |
| 6 | live password change | PASS | PW1 401, PW2 200 |
| 7 | restart invalidates tokens | PASS | old token 401, hash survived, fresh unlock 200 |
| B | UI locked mode | PASS | playwright green — 6 passed (3.0s) |
| 8 | live --clear | PASS | open mode, ungated probe 200, unlock 400 |
| D | UI open mode | PASS | playwright green — 1 passed (843ms) |
| 9 | CLI validation | PASS | too-short + mismatch rejected, no hash written |
| F | cleanup | PASS | final state open mode, ungated probe 200 |

The first run (14/22) caught two test-tooling bugs — an unquoted JSON body in
the runner's `unlock` helper and an untrimmed-whitespace regex in the spec —
which were fixed before the passing run; a third (a `req()` variable clobbering
scenario 4's token) was caught on the next run and fixed the same way. That is
the battle test working as intended: it reports tooling defects instead of
papering over them.

Raw summary block from the passing run (verbatim):

```
----------------------------------------
22/22 passed
final auth_enabled: false
last events:
17:42:23 PASS B ui locked mode
17:42:23 PASS 8 live --clear
17:42:25 PASS D ui open mode
17:42:26 PASS 9 cli validation
17:42:26 PASS F cleanup
```

## Left-behind state

Open mode, no tokens, no setting changed. A handful of `auth_unlock_failed`
events remain in the event log (evidence; the events table is unbounded, so
they persist until cleared manually).
