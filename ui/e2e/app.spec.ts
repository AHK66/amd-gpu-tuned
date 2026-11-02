import { expect, test, type Page } from "@playwright/test";

const SLOT = "1002:7550-148c:2435-0000:0f:00.0";

async function createPresetNamed(page: Page, name: string, powerCap: string) {
  await page.goto("/#/presets");
  await page.getByTestId("new-preset-btn").click();
  await page.getByTestId("preset-name").fill(name);
  await page.getByTestId("power-cap").fill(powerCap);
  await page.getByTestId("save-btn").click();
  await expect(page.getByRole("button", { name, exact: true })).toBeVisible();
}

async function deletePresetByName(page: Page, name: string) {
  await page.goto("/#/presets");
  const row = page.locator("tr").filter({ hasText: name }).last();
  // Row actions are opacity-0/pointer-events-none until the row is hovered.
  await row.locator("td").last().hover();
  await row.getByLabel("delete").click();
  await page.getByRole("dialog").getByRole("button", { name: "delete" }).click();
  // The presets page polls every 5 s; allow a full cycle for the row to disappear.
  await expect(page.getByRole("button", { name, exact: true })).toBeHidden({ timeout: 12_000 });
}

test("dashboard renders the fixture GPU with zero external requests", async ({ page }) => {
  const external: string[] = [];
  page.on("request", (r) => {
    const u = new URL(r.url());
    if (!(u.hostname === "127.0.0.1" && u.port === "8787")) external.push(r.url());
  });
  await page.goto("/");
  await expect(page.getByTestId("nav-dashboard")).toBeVisible();
  await expect(page.getByTestId("gpu-tile")).toBeVisible();
  await expect(page.getByTestId("feed-lamp")).toBeVisible();
  // Spec DoD: zero external network requests at runtime.
  expect(external).toEqual([]);
});

test("card box knob input shows a short range tooltip on hover", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("gpu-tile")).toBeVisible();
  await page.getByTestId("tile-power-cap").hover();
  await expect(page.getByRole("tooltip", { name: "Power cap (0 .. 340 W)" })).toBeVisible();
});

test("sidebar collapses and persists", async ({ page }) => {
  await page.goto("/");
  const toggle = page.getByTestId("sidebar-toggle");
  await toggle.click();
  await expect(page.getByTestId("nav-dashboard")).toBeVisible();
  // After collapse, the wordmark text is hidden (icon rail only).
  await expect(page.getByText("amd-gpu-tuned", { exact: true })).toBeHidden();
});

test("built-in presets are seeded and badged", async ({ page }) => {
  await page.goto("/#/presets");
  await expect(
    page.getByRole("button", { name: "R9700 220W -70mV 1350MHz VRAM" }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "R9700 210W -70mV" })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "R9700 220W -70mV", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "R9700 225W -70mV" })).toBeVisible();
  await expect(page.getByTestId("preset-badge-builtin").first()).toBeVisible();
});

test("preset: create → apply from card box → default reverts; cleanup", async ({ page }) => {
  await createPresetNamed(page, "e2e", "250");
  try {
    // Apply from the dashboard card box by selecting it in the combobox.
    await page.goto("/");
    await page.getByTestId("tile-preset").click();
    await page.getByTestId("tile-preset-list").getByRole("button", { name: "e2e", exact: true }).click();
    await expect(page.getByTestId("tile-power-cap")).toHaveValue("250");
    // Back to stock via the `default` pseudo-preset (no revert button).
    // Fields fall back to stock values: `stockCap() = cap_default_w ?? cap_max_w`;
    // the e2e fixture has `power1_cap_default` = 304 W, so the field shows "304".
    await page.getByTestId("tile-preset").click();
    await page.getByTestId("tile-preset-opt-default").click();
    await expect(page.getByTestId("tile-power-cap")).toHaveValue("304");
  } finally {
    await deletePresetByName(page, "e2e");
  }
});

test("card box: built-in fields are read-only", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("tile-preset").click();
  await page
    .getByTestId("tile-preset-list")
    .getByRole("button", { name: "R9700 210W -70mV (built-in)" })
    .click();
  await expect(page.getByTestId("tile-power-cap")).toHaveValue("210");
  await expect(page.getByTestId("tile-power-cap")).toBeDisabled();
  await expect(page.getByTestId("tile-voltage")).toBeDisabled();
});

test("card box: out-of-range field shows inline error and does not apply", async ({ page }) => {
  await createPresetNamed(page, "e2e-val", "250");
  try {
    await page.goto("/");
    await page.getByTestId("tile-preset").click();
    await page
      .getByTestId("tile-preset-list")
      .getByRole("button", { name: "e2e-val", exact: true })
      .click();
    await expect(page.getByTestId("tile-power-cap")).toHaveValue("250");
    // Type an out-of-range sclk value and confirm with Enter.
    await page.getByTestId("tile-sclk").fill("9999");
    await page.getByTestId("tile-sclk").press("Enter");
    // The field shows an inline error (red border class + "range: …" message)...
    await expect(page.getByTestId("tile-sclk")).toHaveClass(/border-danger/);
    await expect(page.getByText(/^range: /)).toBeVisible();
    // ...and the invalid value was not applied: other fields are untouched
    // and the combo is unchanged (no transient clone created).
    await expect(page.getByTestId("tile-power-cap")).toHaveValue("250");
    await expect(page.getByTestId("tile-preset")).toHaveValue("e2e-val");
  } finally {
    // Delete is blocked while a GPU is on the preset — revert to stock first.
    await page.goto("/");
    await page.getByTestId("tile-preset").click();
    await page.getByTestId("tile-preset-opt-default").click();
    // Wait for the server-side revert to land before deleting the preset.
    await expect(page.getByTestId("tile-preset")).toHaveValue("stock", { timeout: 12_000 });
    await deletePresetByName(page, "e2e-val");
  }
});

test("card box: committed field value survives telemetry ticks", async ({ page }) => {
  await createPresetNamed(page, "e2e-lag", "250");
  try {
    await page.goto("/");
    await page.getByTestId("tile-preset").click();
    await page
      .getByTestId("tile-preset-list")
      .getByRole("button", { name: "e2e-lag", exact: true })
      .click();
    await page.getByTestId("tile-power-cap").fill("255");
    await page.getByTestId("tile-power-cap").press("Enter");
    await expect(page.getByTestId("tile-power-cap")).toHaveValue("255");
    // Survive >= 2 telemetry ticks (~1 s each) without the value jumping back.
    await page.waitForTimeout(2500);
    await expect(page.getByTestId("tile-power-cap")).toHaveValue("255");
  } finally {
    // Delete is blocked while a GPU is on the preset — revert to stock first.
    await page.goto("/");
    await page.getByTestId("tile-preset").click();
    await page.getByTestId("tile-preset-opt-default").click();
    // Post-reload seed re-derives the transient divergence, so picking stock
    // raises the Unsaved-changes switch modal — dismiss it to proceed.
    await page.getByTestId("tile-switch-dismiss").click();
    // Wait for the server-side revert to land before deleting the preset.
    await expect(page.getByTestId("tile-preset")).toHaveValue("stock", { timeout: 12_000 });
    await deletePresetByName(page, "e2e-lag");
  }
});

test("card box: committed field value survives a page reload (applied, not saved preset)", async ({ page }) => {
  // The live box can run multiple GPUs; this test drives the first tile only.
  const tile = page.getByTestId("gpu-tile").first();
  await createPresetNamed(page, "e2e-reload", "250");
  try {
    await page.goto("/");
    await tile.getByTestId("tile-preset").click();
    await tile
      .getByTestId("tile-preset-list")
      .getByRole("button", { name: "e2e-reload", exact: true })
      .click();
    await expect(tile.getByTestId("tile-power-cap")).toHaveValue("250");
    // Transient edit: applied to the card, NOT persisted to the preset.
    await tile.getByTestId("tile-power-cap").fill("255");
    await tile.getByTestId("tile-power-cap").press("Enter");
    await expect(tile.getByTestId("tile-power-cap")).toHaveValue("255");
    // Real GPU writes are slower than the fake-sysfs fixture: the server-side
    // apply of the transient is still in flight here. Wait for it to land in
    // the recorded state before reloading — a reload mid-apply aborts the
    // in-flight write and the recorded value never reaches 255.
    await expect
      .poll(
        async () => {
          const st = await (await page.request.get("/api/status")).json();
          return st.active_state?.settings?.[st.gpus[0].slot]?.power_cap_w;
        },
        { timeout: 30_000 },
      )
      .toBe(255);
    // Close and reopen the web front end: the tile must show the value the
    // card is running (the applied 255), not the saved preset value (250).
    await page.reload();
    await expect(tile.getByTestId("tile-preset")).toHaveValue("e2e-reload");
    await expect(tile.getByTestId("tile-power-cap")).toHaveValue("255");
    // The transient divergence (applied 255 ≠ saved 250) is re-derived from
    // the per-slot applied state: marker + Save affordance survive the reload.
    await expect(tile.getByTestId("tile-lamp-unsaved")).toBeVisible();
    await expect(tile.getByTestId("tile-diverged-power-cap")).toBeVisible();
    await expect(tile.getByTestId("tile-preset-save")).toBeEnabled();
    // Only the edited knob is diverged: the preset leaves sclk/voltage/mclk
    // unset (null), so their concrete applied values (0 / stock) are NOT a
    // divergence — no dots on the untouched fields.
    await expect(tile.getByTestId("tile-diverged-sclk")).toBeHidden();
    await expect(tile.getByTestId("tile-diverged-voltage")).toBeHidden();
    await expect(tile.getByTestId("tile-diverged-mclk")).toBeHidden();
  } finally {
    // Delete is blocked while a GPU is on the preset — revert to stock first.
    await page.goto("/");
    await tile.getByTestId("tile-preset").click();
    await tile.getByTestId("tile-preset-opt-default").click();
    // The re-derived transient divergence (post-reload seed) makes the tile
    // "modified", so picking stock raises the Unsaved-changes switch modal —
    // dismiss (discard) it so the revert to stock can proceed.
    await tile.getByTestId("tile-switch-dismiss").click();
    // Wait for the server-side revert to land (status polls every 5 s) so the
    // presets page doesn't see a stale "in use" source and block the delete.
    await expect(tile.getByTestId("tile-preset")).toHaveValue("stock", { timeout: 12_000 });
    await deletePresetByName(page, "e2e-reload");
  }
});

test("setup checklist renders the fixture GPU", async ({ page }) => {
  await page.goto("/#/setup");
  await expect(page.getByTestId("setup-ready")).toBeVisible();
  await expect(page.getByText(SLOT)).toBeVisible();
});
