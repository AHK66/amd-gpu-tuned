#!/bin/sh
# Live-daemon battle test for the write-endpoint password feature.
# Usage: sh scripts/auth-battle-test.sh   (~3 min; leaves the box in open mode)
#
# Prereqs: daemon running (sh scripts/restart-amd-gpu-tuned.sh), box in open
# mode (auth_enabled: false), Playwright chromium installed
# (cd ui && npx playwright install chromium).
#
# SAFETY: every gated-write probe is a ring-window no-op (writes the current
# value back) — no GPU hardware writes. The only privileged step is `sudo -n`
# on the daemon binary (existing NOPASSWD rule); the single restart goes
# through scripts/restart-amd-gpu-tuned.sh --no-build.
#
# MID-RUN KILL RECOVERY (the box may be left locked):
#   sudo -n target/release/amd-gpu-tuned set-password --clear   (from the repo root)
# shellcheck disable=SC2181  # rc captured for use in fail messages
set -u
cd "$(dirname "$0")/.." || exit

BASE=http://127.0.0.1:8787
# Absolute path: the sudoers rule is exact-path, and absolute removes any
# dependence on sudo's relative-path resolution (matches restart-amd-gpu-tuned.sh).
BIN="$(pwd)/target/release/amd-gpu-tuned"
PW1=battle-test-pw-1
PW2=battle-test-pw-2
PW3=battle-test-pw-3
PW4=battle-test-pw-4

PASS=0
FAIL=0
EVENTS=""
REQF=$(mktemp)

pass() {
  PASS=$((PASS + 1))
  echo "PASS  $1${2:+ — $2}"
  EVENTS="${EVENTS}$(date +%H:%M:%S) PASS $1
"
}

fail() {
  FAIL=$((FAIL + 1))
  echo "FAIL  $1${2:+ — $2}"
  EVENTS="${EVENTS}$(date +%H:%M:%S) FAIL $1
"
}

summary() {
  echo "----------------------------------------"
  echo "$PASS/$((PASS + FAIL)) passed"
  echo "final auth_enabled: $(auth_enabled 2>/dev/null)"
  echo "last events:"
  printf '%s' "$EVENTS" | tail -5
  rm -f "$REQF"
}
trap summary EXIT

# req METHOD PATH [JSON] [TOKEN] — sets REQS (status) and REQB (body).
# local: these must not clobber same-named vars in the caller (e.g. scenario
# 4's token `b`), since sh functions have no local scope by default.
req() {
  # shellcheck disable=SC3043  # `local` is supported by dash (the runtime sh)
  local m p b t
  m=$1; p=$2; b=${3-}; t=${4-}
  if [ -n "$t" ] && [ -n "$b" ]; then
    REQS=$(curl -s -m 10 -o "$REQF" -w '%{http_code}' -X "$m" "$BASE$p" \
      -H 'content-type: application/json' -H "authorization: Bearer $t" -d "$b")
  elif [ -n "$t" ]; then
    REQS=$(curl -s -m 10 -o "$REQF" -w '%{http_code}' -X "$m" "$BASE$p" \
      -H "authorization: Bearer $t")
  elif [ -n "$b" ]; then
    REQS=$(curl -s -m 10 -o "$REQF" -w '%{http_code}' -X "$m" "$BASE$p" \
      -H 'content-type: application/json' -d "$b")
  else
    REQS=$(curl -s -m 10 -o "$REQF" -w '%{http_code}' -X "$m" "$BASE$p")
  fi
  REQB=$(cat "$REQF")
}

auth_enabled() {
  req GET /api/status
  printf '%s' "$REQB" | grep -o '"auth_enabled":[a-z]*' | sed 's/.*://'
}

ring_min() {
  req GET /api/setup/ring
  printf '%s' "$REQB" | grep -o '"minutes":[0-9]*' | sed 's/.*://'
}

# probe [TOKEN] — POST /api/setup/ring with the current value (no-op).
probe() {
  req POST /api/setup/ring "{\"minutes\": $(ring_min)}" ${1+"$1"}
}

unlock() {  # unlock PASSWORD — sets REQS/REQB
  req POST /api/auth/unlock "{\"password\": \"$1\"}"
}

token_of() {
  printf '%s' "$REQB" | grep -o '"token":"[0-9a-f]*"' | sed 's/"token":"//; s/"$//'
}

ttl_of() {
  printf '%s' "$REQB" | grep -o '"ttl_s":[0-9]*' | sed 's/.*://'
}

# set_pw PASSWORD ('' for --clear) — pty pipe for the rpassword prompts.
# `script -e` propagates the child's exit code.
set_pw() {
  if [ -n "$1" ]; then
    printf '%s\n%s\n' "$1" "$1" | script -qec "sudo -n $BIN set-password" /dev/null
  else
    sudo -n "$BIN" set-password --clear
  fi
}

wait_daemon() {  # wait_daemon TIMEOUT_S
  i=0
  while [ "$i" -lt "$1" ]; do
    req GET /api/status
    [ "$REQS" = 200 ] && return 0
    sleep 2
    i=$((i + 2))
  done
  return 1
}

echo "=== auth battle test ==="
echo "PW1=$PW1"
echo "PW2=$PW2"
echo "recovery: sudo -n $BIN set-password --clear"
echo

# --- 0 preflight -------------------------------------------------------------
req GET /api/status
if [ "$REQS" != 200 ]; then
  echo "ABORT: daemon not reachable at $BASE"
  exit 1
fi
ae=$(auth_enabled)
if [ "$ae" != "false" ]; then
  echo "ABORT: box not in open mode (auth_enabled: $ae) — run: sudo -n $BIN set-password --clear"
  exit 1
fi
probe
if [ "$REQS" = 200 ]; then
  pass "0 preflight" "daemon up, open mode, ungated probe 200"
else
  fail "0 preflight" "ungated probe -> $REQS"
fi

# --- 1 live set-password -------------------------------------------------------
set_pw "$PW1"
rc=$?
if [ "$rc" -eq 0 ] && [ "$(auth_enabled)" = "true" ]; then
  pass "1 set-password" "exit 0, auth_enabled true, no restart"
else
  fail "1 set-password" "exit $rc, auth_enabled $(auth_enabled)"
fi
probe
if [ "$REQS" = 401 ] && [ "$REQB" = '{"error":"authentication required"}' ]; then
  pass "1 gated probe" "401 + exact body"
else
  fail "1 gated probe" "$REQS $REQB"
fi
for r in status telemetry presets drift events setup exporter/metrics; do
  req GET "/api/$r"
  if [ "$REQS" = 200 ]; then
    pass "1 read /api/$r" "200"
  else
    fail "1 read /api/$r" "$REQS"
  fi
done
unlock "$PW1"
tok=$(token_of)
ttl=$(ttl_of)
if [ "$REQS" = 200 ] && printf '%s' "$tok" | grep -Eq '^[0-9a-f]{64}$' && [ "$ttl" = 1800 ]; then
  pass "1 unlock" "token 64-hex, ttl_s 1800"
else
  fail "1 unlock" "$REQS tok=$tok ttl=$ttl"
fi

# --- 2 wrong password ------------------------------------------------------------
unlock "wrong-password-1"
us=$REQS
req GET /api/events
ev=$REQB
if [ "$us" = 401 ] && printf '%s' "$ev" | grep -q 'auth_unlock_failed'; then
  pass "2 wrong password" "401 + auth_unlock_failed event"
else
  fail "2 wrong password" "unlock=$us events-match=$(printf '%s' "$ev" | grep -c 'auth_unlock_failed')"
fi

# --- 3 token gates writes ----------------------------------------------------------
m0=$(ring_min)
probe "$tok"; r1=$REQS
probe;        r2=$REQS
m1=$(ring_min)
if [ "$r1" = 200 ] && [ "$r2" = 401 ] && [ "$m0" = "$m1" ]; then
  pass "3 token gates writes" "200 with token, 401 without, ring unchanged ($m0)"
else
  fail "3 token gates writes" "with=$r1 without=$r2 ring $m0->$m1"
fi

# --- 4 concurrent unlocks -------------------------------------------------------------
unlock "$PW1"; a=$(token_of)
unlock "$PW1"; b=$(token_of)
probe "$a"; ra=$REQS
probe "$b"; rb=$REQS
if [ -n "$a" ] && [ -n "$b" ] && [ "$a" != "$b" ] && [ "$ra" = 200 ] && [ "$rb" = 200 ]; then
  pass "4 concurrent unlocks" "A != B, both gate"
else
  fail "4 concurrent unlocks" "probeA=$ra probeB=$rb distinct=$([ "$a" != "$b" ] && echo yes || echo no)"
fi

# --- 5 global lock ---------------------------------------------------------------------
req POST /api/auth/lock '{}' "$a"
rl=$REQS; lk=$REQB
probe "$a"; ra=$REQS
probe "$b"; rb=$REQS
req POST /api/auth/lock '{}'
ru=$REQS
if [ "$rl" = 200 ] && [ "$lk" = '{"locked":true}' ] && [ "$ra" = 401 ] && [ "$rb" = 401 ] && [ "$ru" = 401 ]; then
  pass "5 global lock" "lock 200 {locked:true}, A+B dead, ungated lock 401"
else
  fail "5 global lock" "lock=$rl($lk) probeA=$ra probeB=$rb ungated=$ru"
fi

# --- 6 live password change ----------------------------------------------------------------
set_pw "$PW2" > /dev/null 2>&1
unlock "$PW1"; r1=$REQS
unlock "$PW2"; r2=$REQS
if [ "$r1" = 401 ] && [ "$r2" = 200 ]; then
  pass "6 live password change" "PW1 401, PW2 200"
else
  fail "6 live password change" "pw1=$r1 pw2=$r2"
fi

# --- 7 restart invalidates tokens -------------------------------------------------------------
unlock "$PW2"; t=$(token_of)
sh scripts/restart-amd-gpu-tuned.sh --no-build
rc=$?
if [ "$rc" -ne 0 ] || ! wait_daemon 60; then
  fail "7 restart" "daemon did not come back (restart exit $rc)"
else
  probe "$t"; rt=$REQS
  ae=$(auth_enabled)
  unlock "$PW2"; ru=$REQS
  if [ "$rt" = 401 ] && [ "$ae" = "true" ] && [ "$ru" = 200 ]; then
    pass "7 restart invalidates tokens" "old token 401, hash survived, fresh unlock 200"
  else
    fail "7 restart invalidates tokens" "old=$rt auth_enabled=$ae fresh=$ru"
  fi
fi

# --- Phase B: Playwright, locked mode -----------------------------------------------------------
( cd ui && AUTH_LIVE=1 AUTH_TEST_PASSWORD="$PW2" npx playwright test e2e/auth.spec.ts -g "locked mode" )
rc=$?
if [ "$rc" -eq 0 ]; then
  pass "B ui locked mode" "playwright green"
else
  fail "B ui locked mode" "playwright exit $rc"
fi

# --- 8 live --clear -----------------------------------------------------------------------------
set_pw '' > /dev/null 2>&1
ae=$(auth_enabled)
probe; rp=$REQS
unlock "$PW2"; ru=$REQS
if [ "$ae" = "false" ] && [ "$rp" = 200 ] && [ "$ru" = 400 ]; then
  pass "8 live --clear" "open mode, ungated probe 200, unlock 400"
else
  fail "8 live --clear" "auth_enabled=$ae probe=$rp unlock=$ru"
fi

# --- Phase D: Playwright, open mode -----------------------------------------------------------------
( cd ui && AUTH_LIVE=1 AUTH_TEST_PASSWORD="$PW2" npx playwright test e2e/auth.spec.ts -g "open mode" )
rc=$?
if [ "$rc" -eq 0 ]; then
  pass "D ui open mode" "playwright green"
else
  fail "D ui open mode" "playwright exit $rc"
fi

# --- 9 CLI validation ------------------------------------------------------------------------------
printf '%s\n%s\n' short short | script -qec "sudo -n $BIN set-password" /dev/null
r1=$?
printf '%s\n%s\n' "$PW3" "$PW4" | script -qec "sudo -n $BIN set-password" /dev/null
r2=$?
ae=$(auth_enabled)
unlock "whatever"; ru=$REQS
if [ "$r1" -ne 0 ] && [ "$r2" -ne 0 ] && [ "$ae" = "false" ] && [ "$ru" = 400 ]; then
  pass "9 cli validation" "too-short + mismatch rejected, no hash written"
else
  fail "9 cli validation" "short=$r1 mismatch=$r2 auth_enabled=$ae unlock=$ru"
fi

# --- Phase F: cleanup assert (always runs) ----------------------------------------------------------------
if [ "$(auth_enabled)" = "true" ]; then
  set_pw '' > /dev/null 2>&1
fi
ae=$(auth_enabled)
probe
if [ "$ae" = "false" ] && [ "$REQS" = 200 ]; then
  pass "F cleanup" "final state open mode, ungated probe 200"
else
  fail "F cleanup" "auth_enabled=$ae probe=$REQS"
fi
