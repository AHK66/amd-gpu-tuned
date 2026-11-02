# AGENTS.md

## Daemon operations

The amd-gpu-tuned daemon runs as **root** on `127.0.0.1:8787` (log: `/tmp/amd-gpu-tuned.log`), started through NOPASSWD sudoers rules. The UI is embedded in the binary at compile time (`ui/dist` via rust-embed), so a UI change requires a rebuild.

- **Rebuild + restart:** `sh scripts/restart-amd-gpu-tuned.sh` (add `--no-build` to restart only).
- **NEVER run `sudo pkill -f target/release/amd-gpu-tuned` inline.** The sudoers rule allows only that exact pkill invocation (no `pkill -x`, no variants), and `-f` matches full command lines — inline, it matches the calling shell's own command line and **kills your own session**. Always stop the daemon through `scripts/restart-amd-gpu-tuned.sh`, whose outer command line never contains the pattern.
- The daemon's own `sudo` parent process also matches the pkill pattern and is killed along with it — that is intended.

## Tests

- Rust: `cargo test`.
- UI: `cd ui && npm run check && npm test`
