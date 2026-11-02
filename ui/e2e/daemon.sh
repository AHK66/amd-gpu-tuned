#!/bin/sh
# e2e daemon: real binary, embedded SPA, fake sysfs, throwaway data dir.
# Rebuilds stale elements before starting:
#   - ui/dist: the Vite bundle, if any UI source is newer than it
#   - target/release/amd-gpu-tuned: if any Rust source, manifest, or the bundle
#     is newer than it (rust-embed embeds ui/dist at compile time)
set -eu
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

BIN=target/release/amd-gpu-tuned
DIST=ui/dist/index.html
UI_SRCS="ui/src ui/index.html ui/package.json ui/package-lock.json ui/vite.config.ts ui/svelte.config.js ui/tsconfig.json"
RUST_SRCS="src Cargo.toml Cargo.lock"

# 1) bundle: stale if missing or any UI input is newer than the artifact
if [ ! -f "$DIST" ] || [ -n "$(find $UI_SRCS -newer "$DIST" -print -quit 2>/dev/null)" ]; then
  echo "[e2e] ui/dist is stale — rebuilding bundle"
  [ -d ui/node_modules ] || (cd ui && npm install)
  (cd ui && npm run build)
fi

# 2) binary: stale if missing or any input (Rust sources, bundle) is newer
if [ ! -f "$BIN" ] || \
   [ -n "$(find $RUST_SRCS -newer "$BIN" -print -quit 2>/dev/null)" ] || \
   [ "$DIST" -nt "$BIN" ]; then
  echo "[e2e] binary is stale — rebuilding"
  cargo build --release
fi

# throwaway data dir: wipe so re-runs don't hit UNIQUE(name) clashes
rm -rf "$ROOT/target/e2e-data"
exec ./target/release/amd-gpu-tuned \
  --fake-sysfs fixtures/sysfs \
  --data target/e2e-data \
  --port 8787
