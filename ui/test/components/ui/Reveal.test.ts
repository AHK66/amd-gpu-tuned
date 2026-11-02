import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import RevealHarness from "./RevealHarness.svelte";

describe("Reveal", () => {
  it("wraps content in a group container", () => {
    render(RevealHarness);
    const target = screen.getByTestId("reveal-target");
    expect(target.closest(".group")).not.toBeNull();
  });
});
