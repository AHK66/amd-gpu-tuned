import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import SegmentedTabsHarness from "./SegmentedTabsHarness.svelte";

describe("SegmentedTabs", () => {
  it("renders options and selects the active one", async () => {
    render(SegmentedTabsHarness);
    const curve = screen.getByRole("tab", { name: "curve" });
    await fireEvent.click(curve);
    expect(screen.getByRole("tab", { name: "curve" }).className).toContain("bg-surface-2");
  });
});
