#!/bin/sh
# Rebuild (UI + binary) and restart the running amd-gpu-tuned daemon.
# Usage: sh scripts/restart-amd-gpu-tuned.sh [--no-build]
#
# SAFETY: the pkill stop-step MUST stay inside this script. The sudoers rule
# only allows `pkill -f target/release/amd-gpu-tuned`, and -f matches full
# command lines: run it inline from a shell and it matches the calling
# shell's own command line, killing your session. Inside a script the outer
# shell's cmdline is "sh scripts/restart-amd-gpu-tuned.sh", which never matches.
set -eu
cd "$(dirname "$0")/.."

[ "${1:-}" = "--no-build" ] || sh scripts/build.sh

sudo -n /usr/bin/pkill -f target/release/amd-gpu-tuned || true

i=0
while [ $i -lt 50 ] && pgrep -x amd-gpu-tuned >/dev/null 2>&1; do
  sleep 0.2
  i=$((i + 1))
done

setsid sudo -n "$(pwd)/target/release/amd-gpu-tuned" >>/tmp/amd-gpu-tuned.log 2>&1 </dev/null &

i=0
while [ $i -lt 50 ]; do
  curl -s -m 1 http://127.0.0.1:8787/api/telemetry >/dev/null 2>&1 && break
  sleep 0.2
  i=$((i + 1))
done
curl -s -m 2 http://127.0.0.1:8787/api/telemetry >/dev/null 2>&1 || {
  echo "daemon did not come up — see /tmp/amd-gpu-tuned.log" >&2
  exit 1
}
echo "amd-gpu-tuned restarted (log: /tmp/amd-gpu-tuned.log)"
