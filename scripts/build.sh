#!/bin/sh
# Build the self-contained amd-gpu-tuned binary:
#   1. build the SPA (ui/dist)
#   2. cargo build --release (rust-embed compiles ui/dist into the binary)
set -eu
cd "$(dirname "$0")/.."

(cd ui && npm ci && npm run build)
cargo build --release

echo "built: $(pwd)/target/release/amd-gpu-tuned (SPA embedded)"
