import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import CardHarness from "./CardHarness.svelte";

describe("Card", () => {
  it("renders the title snippet and body", () => {
    render(CardHarness);
    expect(screen.getByText("Header")).toBeVisible();
    expect(screen.getByText("body content")).toBeVisible();
  });
});
