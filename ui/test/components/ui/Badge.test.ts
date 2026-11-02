import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import BadgeHarness from "./BadgeHarness.svelte";

describe("Badge", () => {
  it("renders the label", () => {
    render(BadgeHarness);
    expect(screen.getByText("label")).toBeVisible();
  });

  it("danger tone uses the danger text token", () => {
    render(BadgeHarness, { props: { tone: "danger" } });
    expect(screen.getByText("label").className).toContain("text-danger");
  });
});
