import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  retries: 0,
  use: { baseURL: "http://127.0.0.1:8787" },
  webServer: {
    command: "sh e2e/daemon.sh",
    url: "http://127.0.0.1:8787/",
    // `daemon.sh` runs `cargo build --release` inline; the first (cold) build
    // can exceed 5 min, so leave headroom.
    timeout: 600_000,
    reuseExistingServer: true,
  },
});
