import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import FormHarness from "./FormHarness.svelte";

describe("form controls", () => {
  it("Field renders its label", () => {
    render(FormHarness);
    expect(screen.getByText("name")).toBeVisible();
  });

  it("Toggle is a switch input and toggles on click", async () => {
    render(FormHarness);
    const t = screen.getByTestId("toggle");
    expect(t).toBeChecked();
    expect(t.getAttribute("role")).toBe("switch");
    await fireEvent.click(t);
    expect(t).not.toBeChecked();
  });

  it("Field associates its label with the control", () => {
    render(FormHarness);
    const label = screen.getByText("name").closest("label")!;
    const input = screen.getByTestId("name-input");
    expect(label.getAttribute("for")).toBeTruthy();
    expect(label.getAttribute("for")).toBe(input.id);
  });

  it("TextInput accepts input", async () => {
    render(FormHarness);
    const input = screen.getByTestId("name-input");
    await fireEvent.input(input, { target: { value: "abc" } });
    expect((input as HTMLInputElement).value).toBe("abc");
  });
});
