import { expect, test, type Page } from "@playwright/test";

// Live-daemon auth battle test — driven by scripts/auth-battle-test.sh with
// AUTH_LIVE=1 + AUTH_TEST_PASSWORD. Under the normal fake-sysfs `npm run e2e`
// every test here skips, keeping that suite green.
const live = process.env.AUTH_LIVE === "1";
const PW = process.env.AUTH_TEST_PASSWORD ?? "";
const SKIP = "live-daemon auth test — run via scripts/auth-battle-test.sh";

// lock-indicator renders only when the sidebar is expanded (the expanded
// state is persisted in localStorage) and after the status poll sets
// authState.enabled. Expand if collapsed, then wait for it.
async function ensureSidebarExpanded(page: Page) {
  const toggle = page.getByTestId("sidebar-toggle");
  if ((await toggle.getAttribute("aria-label")) === "expand sidebar") {
    await toggle.click();
  }
  await expect(page.getByTestId("lock-indicator")).toBeVisible({ timeout: 10_000 });
}

// Deterministic gated-write no-op: read the daemon's current ring minutes
// (ungated read) and commit that exact value back, so nothing changes.
// commitNow() always POSTs (no unchanged-value skip), so in locked mode the
// POST 401s and opens the unlock dialog; in unlocked mode it writes the
// value back (a no-op).
async function commitRing(page: Page) {
  await page.goto("/#/setup");
  const res = await page.request.get("/api/setup/ring");
  const { minutes } = (await res.json()) as { minutes: number };
  const input = page.getByTestId("ring-minutes");
  await expect(input).toBeVisible();
  await input.fill(String(minutes));
  await input.press("Enter");
}

async function unlockViaDialog(page: Page, password: string) {
  await page.getByTestId("unlock-password").fill(password);
  await page.getByRole("button", { name: "unlock", exact: true }).click();
}

test.describe("locked mode", () => {
  test("gated write opens the unlock dialog", async ({ page }) => {
    test.skip(!live, SKIP);
    await commitRing(page);
    await expect(page.getByRole("dialog")).toBeVisible();
    await expect(page.getByRole("heading", { name: "Unlock writes" })).toBeVisible();
    await expect(page.getByTestId("unlock-password")).toBeVisible();
  });

  test("wrong password shows the error", async ({ page }) => {
    test.skip(!live, SKIP);
    await commitRing(page);
    await unlockViaDialog(page, "wrong-password-1");
    await expect(page.getByTestId("unlock-error")).toHaveText("wrong password");
    // Dialog stays open after a failed attempt.
    await expect(page.getByTestId("unlock-password")).toBeVisible();
  });

  test("correct password unlocks and the write lands", async ({ page }) => {
    test.skip(!live, SKIP);
    await commitRing(page);
    await unlockViaDialog(page, PW);
    await expect(page.getByTestId("unlock-password")).toBeHidden();
    // Second commit goes through without re-opening the dialog.
    await commitRing(page);
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await ensureSidebarExpanded(page);
    await expect(page.getByTestId("lock-indicator")).toHaveText(/unlocked/);
  });

  test("lock now", async ({ page }) => {
    test.skip(!live, SKIP);
    await commitRing(page);
    await unlockViaDialog(page, PW);
    await ensureSidebarExpanded(page);
    await page.getByTestId("lock-indicator").click();
    await expect(page.getByRole("dialog")).toBeVisible();
    await expect(page.getByRole("heading", { name: "Lock writes?" })).toBeVisible();
    await page.getByRole("button", { name: "lock now" }).click();
    // ^ ?locked$ (not /locked/) so it cannot match the "unlocked" state —
    // the leading ? handles the icon's trailing space (Playwright collapses
    // whitespace but does not trim).
    await expect(page.getByTestId("lock-indicator")).toHaveText(/^ ?locked$/);
    // The next write re-opens the dialog.
    await commitRing(page);
    await expect(page.getByTestId("unlock-password")).toBeVisible();
  });

  test("page reload drops the token", async ({ page }) => {
    test.skip(!live, SKIP);
    await commitRing(page);
    await unlockViaDialog(page, PW);
    await ensureSidebarExpanded(page);
    await expect(page.getByTestId("lock-indicator")).toHaveText(/unlocked/);
    await page.reload();
    await ensureSidebarExpanded(page);
    await expect(page.getByTestId("lock-indicator")).toHaveText(/^ ?locked$/);
  });

  test("no token in page storage", async ({ page }) => {
    test.skip(!live, SKIP);
    await commitRing(page);
    await unlockViaDialog(page, PW);
    await expect(page.getByTestId("unlock-password")).toBeHidden();
    const entries = [
      ...(await page.evaluate(() => Object.entries(localStorage))),
      ...(await page.evaluate(() => Object.entries(sessionStorage))),
    ];
    // No key or value has the bearer-token shape (64 lowercase hex).
    // Pre-existing non-token entries (e.g. amd-gpu-tuned:sidebar) are fine.
    for (const [k, v] of entries) {
      expect(k).not.toMatch(/^[0-9a-f]{64}$/);
      expect(v).not.toMatch(/^[0-9a-f]{64}$/);
    }
  });
});

test.describe("open mode", () => {
  test("setup shows the no-password note", async ({ page }) => {
    test.skip(!live, SKIP);
    await page.goto("/#/setup");
    await expect(page.getByTestId("no-password-note")).toBeVisible();
    await expect(page.getByTestId("lock-indicator")).toHaveCount(0);
  });
});
