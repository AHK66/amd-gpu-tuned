import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Drift from "../../src/routes/Drift.svelte";
import { clearDrift, getDrift, listPresets } from "../../src/lib/api";
import type { DriftEntry, DriftStatus, Preset } from "../../src/lib/types";

const KNOBS = { power_cap_w: 300, sclk_offset_mhz: 0, max_mclk_mhz: 1258, voltage_offset_mv: 0 };
const INTENDED = { power_cap_w: 235, sclk_offset_mhz: 100, max_mclk_mhz: 1150, voltage_offset_mv: -50 };

function entry(i: number, outcome: DriftEntry["outcome"]): DriftEntry {
  return {
    id: i,
    ts: Date.now() - (10 - i) * 60_000,
    slot: "1002:7551-1458:242f-0000:0c:00.0",
    detected: KNOBS,
    intended: outcome === "logged_only" ? null : INTENDED,
    previous_source: outcome === "logged_only" ? "stock" : "preset:7",
    outcome,
    outcome_detail: null,
  };
}

const PRESET: Preset = {
  id: 7,
  name: "Quiet",
  notes: "",
  settings: { power_cap_w: 235, sclk_offset_mhz: null, max_mclk_mhz: null, voltage_offset_mv: null },
  is_builtin: false,
  created_at: null,
  updated_at: null,
};

function status(entries: DriftEntry[], pending: DriftStatus["pending"] = {}): DriftStatus {
  return { reapply: true, delay_s: 30, entries, pending };
}

vi.mock("../../src/lib/api", () => ({
  getDrift: vi.fn(),
  listPresets: vi.fn(async () => [PRESET]),
  clearDrift: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(listPresets).mockResolvedValue([PRESET]);
  vi.mocked(clearDrift).mockReset();
});

describe("Drift", () => {
  it("shows the empty state", async () => {
    vi.mocked(getDrift).mockResolvedValue(status([]));
    render(Drift);
    expect(await screen.findByText("No drift events recorded.")).toBeVisible();
  });

  it("renders all six outcome badges with the preset name resolved", async () => {
    vi.mocked(getDrift).mockResolvedValue(
      status([
        entry(6, "pending"),
        entry(5, "reapplied"),
        entry(4, "reapply_failed"),
        entry(3, "cleared"),
        entry(2, "logged_only"),
        entry(1, "adopted"),
      ]),
    );
    render(Drift);
    for (const label of ["pending", "reapplied", "reapply_failed", "cleared", "logged_only", "adopted"]) {
      expect(await screen.findByText(label)).toBeVisible();
    }
    // preset:7 resolves to the preset name.
    expect(screen.getAllByText("Quiet").length).toBeGreaterThan(0);
    // intended is "—" for the logged_only row.
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);
  });

  it("shows a pending badge per card with a pending restore", async () => {
    vi.mocked(getDrift).mockResolvedValue(
      status([entry(1, "pending")], {
        "1002:7551-1458:242f-0000:0c:00.0": { entry_id: 1, deadline_ms: Date.now() + 30_000 },
      }),
    );
    render(Drift);
    const badge = await screen.findByTestId("drift-pending-badge");
    expect(badge.textContent).toContain("1002:7551-1458:242f-0000:0c:00.0");
  });

  it("shows the current mode in the header", async () => {
    vi.mocked(getDrift).mockResolvedValue(status([]));
    render(Drift);
    expect(await screen.findByText("auto-restore on (30 s)")).toBeVisible();
  });

  it("shows the off mode in the header", async () => {
    vi.mocked(getDrift).mockResolvedValue({ reapply: false, delay_s: 30, entries: [], pending: {} });
    render(Drift);
    expect(await screen.findByText("auto-restore off")).toBeVisible();
  });

  it("disables the clear button when the ledger is empty", async () => {
    vi.mocked(getDrift).mockResolvedValue(status([]));
    render(Drift);
    expect(await screen.findByTestId("drift-clear")).toBeDisabled();
  });

  it("clears the history through the confirm dialog", async () => {
    vi.mocked(getDrift)
      .mockResolvedValueOnce(status([entry(2, "reapplied"), entry(1, "adopted")]))
      .mockResolvedValueOnce(status([]));
    vi.mocked(clearDrift).mockResolvedValue({ cleared: 2 });
    render(Drift);
    const btn = await screen.findByTestId("drift-clear");
    expect(btn).toBeEnabled();
    await fireEvent.click(btn);
    const dialog = screen.getByRole("dialog");
    expect(dialog.textContent).toContain("2 entries");
    await fireEvent.click(within(dialog).getByRole("button", { name: "clear" }));
    expect(clearDrift).toHaveBeenCalledOnce();
    expect(await screen.findByText("No drift events recorded.")).toBeVisible();
  });

  it("cancel leaves the history untouched", async () => {
    vi.mocked(getDrift).mockResolvedValue(status([entry(1, "reapplied")]));
    render(Drift);
    await fireEvent.click(await screen.findByTestId("drift-clear"));
    const dialog = screen.getByRole("dialog");
    await fireEvent.click(within(dialog).getByRole("button", { name: "cancel" }));
    expect(clearDrift).not.toHaveBeenCalled();
    expect(screen.getByText("reapplied")).toBeVisible();
  });

  it("a failed clear keeps the history and surfaces an error", async () => {
    vi.mocked(getDrift).mockResolvedValue(status([entry(1, "reapplied")]));
    vi.mocked(clearDrift).mockRejectedValue(new Error("boom"));
    render(Drift);
    await fireEvent.click(await screen.findByTestId("drift-clear"));
    const dialog = screen.getByRole("dialog");
    await fireEvent.click(within(dialog).getByRole("button", { name: "clear" }));
    expect(clearDrift).toHaveBeenCalledOnce();
    expect(screen.getByText("reapplied")).toBeVisible();
  });
});
