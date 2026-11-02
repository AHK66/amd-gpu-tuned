import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import TooltipHarness from "./TooltipHarness.svelte";

describe("Tooltip", () => {
  it("renders the trigger and wires the tooltip text", () => {
    render(TooltipHarness, { props: { text: "Power limit (W). Lower = cooler, quieter." } });
    expect(screen.getByTestId("tip-trigger")).toBeVisible();
    const tip = screen.getByRole("tooltip");
    expect(tip).toHaveTextContent("Power limit (W). Lower = cooler, quieter.");
    // CSS-only reveal: the text is in the DOM, hidden until hover/focus.
    expect(tip.classList.contains("hidden")).toBe(true);
  });

  it("exposes the tooltip to the trigger via aria-describedby", () => {
    render(TooltipHarness);
    const wrapper = screen.getByTestId("tip-trigger").parentElement!;
    const described = wrapper.getAttribute("aria-describedby");
    expect(described).not.toBeNull();
    expect(document.getElementById(described!)).toBe(screen.getByRole("tooltip"));
  });
});
