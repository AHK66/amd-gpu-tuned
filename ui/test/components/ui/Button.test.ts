import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import ButtonHarness from "./ButtonHarness.svelte";

describe("Button", () => {
  it("renders the label and a leading icon", () => {
    render(ButtonHarness);
    expect(screen.getByText("save")).toBeVisible();
    expect(screen.getByTestId("btn").querySelector("svg")).not.toBeNull();
  });

  it("primary uses the inverted (bg-text) treatment", () => {
    render(ButtonHarness, { props: { variant: "primary" } });
    expect(screen.getByTestId("btn").className).toContain("bg-text");
  });

  it("pill uses the pill radius", () => {
    render(ButtonHarness, { props: { pill: true } });
    expect(screen.getByTestId("btn").className).toContain("rounded-pill");
  });
});
