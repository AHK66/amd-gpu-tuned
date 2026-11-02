import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import EmptyStateHarness from "./EmptyStateHarness.svelte";

describe("EmptyState", () => {
  it("renders icon, heading and subtext", () => {
    const { container } = render(EmptyStateHarness);
    expect(screen.getByText("Waiting")).toBeVisible();
    expect(screen.getByText("no data yet")).toBeVisible();
    expect(container.querySelector("svg")).not.toBeNull();
  });
});
