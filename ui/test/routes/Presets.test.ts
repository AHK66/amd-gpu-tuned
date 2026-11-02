import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Presets from "../../src/routes/Presets.svelte";
import { createPreset, deletePreset, getStatus, listPresets } from "../../src/lib/api";
import { toasts, _resetToastsForTests } from "../../src/lib/store/toast.svelte";
import type { GpuStatic, Status } from "../../src/lib/types";
import { STATIC } from "../logic/thermals.test";

const RANGED: GpuStatic = {
  ...STATIC,
  sclk_offset_range_mhz: [-500, 1000],
  mclk_range_mhz: [97, 1500],
  voltage_offset_range_mv: [-200, 0],
};

const STATUS: Status = {
  ok: true,
  version: "test",
  uptime_ms: 0,
  busy: false,
  privileged: true,
  auth_enabled: false,
  baseline_ts: null,
  baseline_age_ms: null,
  active_state: null,
  active_sources: null,
  gpus: [{ slot: "s", static: RANGED, live: { power_cap_w: null, od: null, fan: null } }],
};

vi.mock("../../src/lib/api", () => ({
  listPresets: vi.fn(async () => []),
  getStatus: vi.fn(async () => STATUS),
  createPreset: vi.fn(async (b: { name: string }) => ({
    id: 1,
    name: b.name,
    notes: "",
    settings: {},
    is_builtin: false,
    created_at: null,
    updated_at: null,
  })),
  updatePreset: vi.fn(async () => ({})),
  deletePreset: vi.fn(async () => null),
  exportPreset: vi.fn(async () => ({})),
}));

describe("Presets save validation", () => {
  beforeEach(() => {
    _resetToastsForTests();
    vi.mocked(createPreset).mockClear();
    vi.mocked(deletePreset).mockClear();
    vi.mocked(listPresets).mockReset();
    vi.mocked(listPresets).mockResolvedValue([]);
    vi.mocked(getStatus).mockReset();
    vi.mocked(getStatus).mockResolvedValue(STATUS);
  });

  const P3: import("../../src/lib/types").Preset = {
    id: 3,
    name: "p3",
    notes: "",
    settings: {
      power_cap_w: null,
      sclk_offset_mhz: null,
      max_mclk_mhz: null,
      voltage_offset_mv: null,
    },
    is_builtin: false,
    created_at: null,
    updated_at: null,
  };

  it("blocks delete when a GPU is on the preset", async () => {
    vi.mocked(listPresets).mockResolvedValue([P3]);
    vi.mocked(getStatus).mockResolvedValue({ ...STATUS, active_sources: { s: "preset:3" } });
    render(Presets);
    await new Promise((r) => setTimeout(r, 0));
    await fireEvent.click(screen.getByRole("button", { name: "delete" }));
    await new Promise((r) => setTimeout(r, 0));
    expect(deletePreset).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog");
    expect(dialog.textContent).toContain("Can't delete");
    expect(dialog.textContent).toContain("p3");
  });

  it("deletes an unused preset", async () => {
    vi.mocked(listPresets).mockResolvedValue([P3]);
    vi.mocked(getStatus).mockResolvedValue({ ...STATUS, active_sources: { s: "stock" } });
    render(Presets);
    await new Promise((r) => setTimeout(r, 0));
    await fireEvent.click(screen.getByRole("button", { name: "delete" }));
    await new Promise((r) => setTimeout(r, 0));
    const dialog = screen.getByRole("dialog");
    await fireEvent.click(within(dialog).getByRole("button", { name: "delete" }));
    await new Promise((r) => setTimeout(r, 0));
    expect(deletePreset).toHaveBeenCalledWith(3);
  });

  it("toolbar has no revert/baseline buttons", async () => {
    render(Presets);
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByTestId("revert-btn")).toBeNull();
    expect(screen.queryByTestId("baseline-btn")).toBeNull();
  });

  it("table has no knobs column", async () => {
    render(Presets);
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByRole("columnheader", { name: "knobs" })).toBeNull();
  });

  it("row actions are not hover-gated", async () => {
    vi.mocked(listPresets).mockResolvedValue([P3]);
    render(Presets);
    await new Promise((r) => setTimeout(r, 0));
    const btn = screen.getByRole("button", { name: "delete" });
    expect(btn.parentElement?.className).not.toContain("opacity-0");
    expect(btn.parentElement?.className).not.toContain("pointer-events-none");
    expect(btn.parentElement?.className).not.toContain("group-hover");
  });

  it("toolbar has no import button", async () => {
    render(Presets);
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByTestId("import-preset-btn")).toBeNull();
  });

  it("rows have no export action", async () => {
    vi.mocked(listPresets).mockResolvedValue([P3]);
    render(Presets);
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByRole("button", { name: "export" })).toBeNull();
  });

  it("columns are ordered actions, name, updated", async () => {
    render(Presets);
    await new Promise((r) => setTimeout(r, 0));
    const headers = screen.getAllByRole("columnheader");
    expect(headers.map((h) => h.textContent?.trim())).toEqual([
      "actions",
      "name",
      "updated",
    ]);
  });

  async function newPresetWithSclk(value: string) {
    render(Presets);
    // Let the mocked getStatus() promise resolve so `firstGpu` (the
    // validation reference) is populated before we interact. The button
    // renders unconditionally, so findByTestId would not guarantee this.
    await new Promise((r) => setTimeout(r, 0));
    await fireEvent.click(screen.getByTestId("new-preset-btn"));
    await fireEvent.input(screen.getByTestId("sclk-offset"), { target: { value } });
    await fireEvent.click(screen.getByTestId("save-btn"));
  }

  it("blocks an out-of-range sclk offset with a toast and no createPreset", async () => {
    await newPresetWithSclk("2000");
    expect(createPreset).not.toHaveBeenCalled();
    expect(
      toasts.some((t) => t.msg === "sclk offset 2000 out of range (-500…1000 MHz)"),
    ).toBe(true);
  });

  it("saves an in-range value", async () => {
    await newPresetWithSclk("100");
    expect(createPreset).toHaveBeenCalledTimes(1);
  });
});
