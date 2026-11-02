import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import PresetEditorHarness from "./PresetEditorHarness.svelte";
import { emptySlotForm } from "../../src/lib/logic/form";
import type { SlotForm } from "../../src/lib/logic/form";

describe("PresetEditor", () => {
  it("renders the single-card knob form", async () => {
    const form: SlotForm = emptySlotForm();
    render(PresetEditorHarness, { props: { name: "p", notes: "", sourceForm: form } });
    expect(screen.getByTestId("preset-card")).toBeVisible();
    expect(screen.getByTestId("power-cap")).toBeVisible();

    await fireEvent.input(screen.getByTestId("power-cap"), { target: { value: "235" } });
    expect(form.power_cap_w).toBe("235");
  });

  it("exposes a tooltip per knob label (description only when the card has no OD ranges)", () => {
    render(PresetEditorHarness, { props: { name: "p", notes: "", sourceForm: emptySlotForm() } });
    // Four tooltips render: power cap, sclk, max mclk, voltage.
    expect(screen.getAllByRole("tooltip").length).toBe(4);
    // STATIC has cap_min 50 / cap_max 350 → range appended; no OD ranges → description only.
    expect(screen.getByText("Power cap (50 .. 350 W)")).toBeVisible();
    expect(screen.getByText("Offset GPU core clock")).toBeVisible();
  });
});
