import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import FieldHarness from "./FieldHarness.svelte";

describe("Field", () => {
  it("renders a plain string label", () => {
    render(FieldHarness);
    expect(screen.getByText("labeled")).toBeVisible();
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("renders a label snippet (tooltip) when provided", () => {
    render(FieldHarness, { props: { withTooltip: true } });
    expect(screen.getByText("labeled")).toBeVisible();
    expect(screen.getByRole("tooltip")).toHaveTextContent("tip text");
    expect(screen.getByTestId("field-input")).toBeVisible();
  });
});
