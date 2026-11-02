import { test, expect } from "@playwright/test";

test.describe("Dashboard charts", () => {
  test("shows temperature and power charts with data", async ({ page }) => {
    await page.goto("/");

    // Wait for GPU tiles to appear (status polled)
    await expect(page.locator("text=Waiting for GPU status")).toBeHidden({ timeout: 10_000 });

    // Chart titles (per-GPU: `Temperatures · <slot>`, `Power & Fan · <slot>`)
    const SLOT = "1002:7550-148c:2435-0000:0f:00.0";
    await expect(page.getByRole("img", { name: `Temperatures · ${SLOT}` })).toBeVisible({ timeout: 10_000 });
    await expect(page.getByRole("img", { name: `Power & Fan · ${SLOT}` })).toBeVisible();

    // Wait for SSE data to populate at least one path per chart
    const tempChart = page.getByRole("img", { name: `Temperatures · ${SLOT}` });
    await expect(async () => {
      expect(await tempChart.locator("path[data-series]").count()).toBeGreaterThan(0);
    }).toPass({ timeout: 10_000 });

    const powerChart = page.getByRole("img", { name: `Power & Fan · ${SLOT}` });
    expect(await powerChart.locator("path[data-series]").count()).toBeGreaterThan(0);

    // Y-axis tick labels exist
    const tickTexts = tempChart.locator("text");
    await expect(tickTexts.first()).toBeVisible();
  });
});
