import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import GpuTile from "../../src/lib/components/GpuTile.svelte";
import { apply, createPreset, deletePreset, revert, updatePreset } from "../../src/lib/api";
import { toasts, _resetToastsForTests } from "../../src/lib/store/toast.svelte";
import { hasPending, resetForTests as resetPending } from "../../src/lib/store/pending.svelte";
import { openModal, closeModal } from "../../src/lib/store/modal.svelte";
import { sample, STATIC } from "../logic/thermals.test";
import type { ApplyReport, GpuStatic, Preset, PresetSettings } from "../../src/lib/types";

vi.mock("../../src/lib/api", () => ({
  apply: vi.fn(async () => ({ ok: true, per_gpu: {} })),
  revert: vi.fn(async () => ({ ok: true, per_gpu: {} })),
  createPreset: vi.fn(
    async (b: { name: string; settings: PresetSettings }) => ({
      id: 42,
      name: b.name,
      notes: "",
      settings: b.settings,
      is_builtin: false,
      created_at: null,
      updated_at: null,
    }),
  ),
  updatePreset: vi.fn(async (id: number, b: { name?: string; settings?: PresetSettings }) => ({
    id,
    name: b.name ?? "x",
    notes: "",
    settings: b.settings,
    is_builtin: false,
    created_at: null,
    updated_at: null,
  })),
  deletePreset: vi.fn(async () => null),
}));

beforeEach(() => {
  closeModal();
  vi.mocked(apply).mockClear();
  vi.mocked(revert).mockClear();
  vi.mocked(updatePreset).mockClear();
  vi.mocked(createPreset).mockClear();
  vi.mocked(deletePreset).mockClear();
  _resetToastsForTests();
  resetPending();
});

const RANGED: GpuStatic = {
  ...STATIC,
  sclk_offset_range_mhz: [-500, 1000],
  mclk_range_mhz: [97, 1500],
  voltage_offset_range_mv: [-200, 0],
};

function settings(p: Partial<PresetSettings> = {}): PresetSettings {
  return {
    power_cap_w: null,
    sclk_offset_mhz: null,
    max_mclk_mhz: null,
    voltage_offset_mv: null,
    ...p,
  };
}

function preset(id: number, name: string, is_builtin = false, p: Partial<PresetSettings> = {}): Preset {
  return { id, name, notes: "", settings: settings(p), is_builtin, created_at: null, updated_at: null };
}

function renderTile(
  props: Partial<{
    sample: ReturnType<typeof sample>;
    ring: ReturnType<typeof sample>[];
    activeSource: string | null;
    presets: Preset[];
    activeSettings: PresetSettings | null;
    busy: boolean;
    stale: boolean;
    drifted?: boolean;
  }> = {},
) {
  return render(GpuTile, {
    props: {
      sample: sample(),
      ring: [],
      staticInfo: RANGED,
      stale: false,
      presets: [],
      busy: false,
      activeSource: null,
      activeSettings: null,
      drifted: false,
      onPresetsChanged: () => {},
      onStatusChanged: () => {},
      ...props,
    },
  });
}

const tick = () => new Promise((r) => setTimeout(r, 0));

async function pickOption(optionTestid: string) {
  await fireEvent.click(screen.getByTestId("tile-preset"));
  await fireEvent.click(screen.getByTestId(optionTestid));
}

async function commitFieldInput(testid: string, value: string) {
  const el = screen.getByTestId(testid);
  await fireEvent.input(el, { target: { value } });
  await fireEvent.keyDown(el, { key: "Enter" });
}

describe("GpuTile", () => {
  it("renders power, cap headroom, and clocks", () => {
    renderTile();
    expect(screen.getByText("200 W")).toBeVisible();
    expect(screen.getByText(/100 W headroom/)).toBeVisible();
  });

  it("shows free VRAM next to the vram temp, red when nearly full", () => {
    // 32768 - 32256 = 512 MB = 0.5 GB free → 1.56% → critical
    renderTile({ sample: sample({ vram_c: 56, vram_used_mb: 32256, vram_total_mb: 32768 }) });
    expect(screen.getByText("free")).toBeVisible();
    expect(screen.getByText("0.5GB").className).toContain("text-danger");
  });

  it("shows free VRAM amber in the 5..20% band", () => {
    // 32768 - 26624 = 6144 MB = 6.0 GB free → 18.75% → warn
    renderTile({ sample: sample({ vram_c: 56, vram_used_mb: 26624, vram_total_mb: 32768 }) });
    expect(screen.getByText("6.0GB").className).toContain("text-warn");
  });

  it("shows free VRAM green when plenty is free", () => {
    // 32768 - 8192 = 24576 MB = 24 GB free → 75% → ok
    renderTile({ sample: sample({ vram_c: 50, vram_used_mb: 8192, vram_total_mb: 32768 }) });
    expect(screen.getByText("24GB").className).toContain("text-success");
  });

  it("omits the free VRAM readout when total/used are unknown", () => {
    renderTile({ sample: sample({ vram_c: 50 }) });
    expect(screen.queryByText("free")).toBeNull();
  });

  it("colors the driving sensor red within 5 °C of slowdown", () => {
    const { container } = render(GpuTile, {
      props: {
        sample: sample({ edge_c: 87 }),
        staticInfo: RANGED,
        stale: false,
        presets: [],
        busy: false,
        activeSource: null,
        activeSettings: null,
        onPresetsChanged: () => {},
        onStatusChanged: () => {},
      },
    });
    expect(container.querySelector(".text-danger")).not.toBeNull();
  });

  it("colors amber in the 5..15 °C band", () => {
    const { container } = render(GpuTile, {
      props: {
        sample: sample({ edge_c: 80 }),
        staticInfo: RANGED,
        stale: false,
        presets: [],
        busy: false,
        activeSource: null,
        activeSettings: null,
        onPresetsChanged: () => {},
        onStatusChanged: () => {},
      },
    });
    expect(container.querySelector('[data-testid="tile-sensors"] .text-warn')).not.toBeNull();
  });

  it("shows the throttle lamp when throttle bits are set", () => {
    render(GpuTile, {
      props: {
        sample: sample({ throttle_bits: ["THROTTLE_THERMAL"] }),
        staticInfo: RANGED,
        stale: false,
        presets: [],
        busy: false,
        activeSource: null,
        activeSettings: null,
        onPresetsChanged: () => {},
        onStatusChanged: () => {},
      },
    });
    expect(screen.getByTestId("tile-lamp-throttled")).toBeVisible();
  });

  it("lamps are unmounted when their state is off", () => {
    renderTile();
    expect(screen.queryByTestId("tile-lamp-throttled")).toBeNull();
    expect(screen.queryByTestId("tile-lamp-stale")).toBeNull();
  });

  it("unsaved lamp is absent for stock", () => {
    renderTile();
    expect(screen.queryByTestId("tile-lamp-unsaved")).toBeNull();
  });

  it("unsaved lamp appears after a committed edit to an editable preset", async () => {
    renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210 })],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    expect(screen.queryByTestId("tile-lamp-unsaved")).toBeNull();
    await commitFieldInput("tile-power-cap", "215");
    await tick();
    expect(screen.getByTestId("tile-lamp-unsaved")).toBeVisible();
  });

  it("unsaved lamp is absent for a builtin preset", () => {
    renderTile({
      activeSource: "preset:7",
      presets: [preset(7, "b7", true, { power_cap_w: 210 })],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    expect(screen.queryByTestId("tile-lamp-unsaved")).toBeNull();
  });

  it("general lamps show and hide with their state", () => {
    const { rerender } = renderTile({
      stale: true,
      drifted: true,
      sample: sample({ throttle_bits: ["THROTTLE_THERMAL"] }),
    });
    expect(screen.getByTestId("tile-lamp-stale")).toBeVisible();
    expect(screen.getByTestId("tile-lamp-drifted")).toBeVisible();
    expect(screen.getByTestId("tile-lamp-throttled")).toBeVisible();
    rerender({ stale: false, drifted: false, sample: sample() });
    expect(screen.queryByTestId("tile-lamp-stale")).toBeNull();
    expect(screen.queryByTestId("tile-lamp-drifted")).toBeNull();
    expect(screen.queryByTestId("tile-lamp-throttled")).toBeNull();
  });

  it("lamps render in canonical order (preset subset: stale, drifted, unsaved, throttled)", async () => {
    const { container } = renderTile({
      stale: true,
      drifted: true,
      sample: sample({ throttle_bits: ["THROTTLE_THERMAL"] }),
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210 })],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    await commitFieldInput("tile-power-cap", "215");
    await tick();
    const order = [...container.querySelectorAll('[data-testid^="tile-lamp-"]')].map((el) =>
      el.getAttribute("data-testid"),
    );
    expect(order).toEqual(["tile-lamp-stale", "tile-lamp-drifted", "tile-lamp-unsaved", "tile-lamp-throttled"]);
  });

  it("lamps render in canonical order (external subset: stale, outside, drifted, throttled)", () => {
    const { container } = renderTile({
      stale: true,
      drifted: true,
      sample: sample({ throttle_bits: ["THROTTLE_THERMAL"] }),
      activeSource: "external",
      activeSettings: settings({ power_cap_w: 235 }),
    });
    const order = [...container.querySelectorAll('[data-testid^="tile-lamp-"]')].map((el) =>
      el.getAttribute("data-testid"),
    );
    expect(order).toEqual(["tile-lamp-stale", "tile-lamp-outside", "tile-lamp-drifted", "tile-lamp-throttled"]);
  });

  it("each active lamp exposes its tooltip text", () => {
    renderTile({
      stale: true,
      drifted: true,
      sample: sample({ throttle_bits: ["THROTTLE_THERMAL"] }),
      activeSource: "external",
    });
    expect(screen.getByText("No fresh telemetry (link stale)")).toHaveTextContent(
      "No fresh telemetry (link stale)",
    );
    expect(screen.getByText("Settings changed outside amd-gpu-tuned")).toHaveTextContent(
      "Settings changed outside amd-gpu-tuned",
    );
    expect(screen.getByText("Value drifted — auto-restoring")).toHaveTextContent(
      "Value drifted — auto-restoring",
    );
    expect(screen.getByText("GPU is throttling (thermal / power)")).toHaveTextContent(
      "GPU is throttling (thermal / power)",
    );
  });

  it("shows a stale indicator without a sample", () => {
    render(GpuTile, {
      props: {
        sample: null,
        staticInfo: RANGED,
        stale: true,
        presets: [],
        busy: false,
        activeSource: null,
        activeSettings: null,
        onPresetsChanged: () => {},
        onStatusChanged: () => {},
      },
    });
    expect(screen.getByTestId("tile-lamp-stale")).toBeVisible();
  });

  it("renders stock values — no empty fields (mclk excepted before any sample)", () => {
    renderTile();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("300");
    expect(screen.getByTestId("tile-sclk")).toHaveValue("0");
    expect(screen.getByTestId("tile-voltage")).toHaveValue("0");
    expect(screen.getByTestId("tile-mclk")).toHaveValue("");
  });

  it("renders effective values over stock fallbacks", () => {
    renderTile({
      activeSettings: settings({ power_cap_w: 220, voltage_offset_mv: -70, max_mclk_mhz: 1350 }),
    });
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("220");
    expect(screen.getByTestId("tile-sclk")).toHaveValue("0");
    expect(screen.getByTestId("tile-voltage")).toHaveValue("-70");
    expect(screen.getByTestId("tile-mclk")).toHaveValue("1350");
  });

  it("a telemetry tick does not wipe a typed value", async () => {
    const { rerender } = renderTile();
    await fireEvent.input(screen.getByTestId("tile-power-cap"), { target: { value: "21" } });
    rerender({ sample: sample() });
    await tick();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("21");
  });

  it("a committed value stays visible until activeSettings catches up", async () => {
    const { rerender } = renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210 })],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    await pickOption("tile-preset-opt-3");
    await commitFieldInput("tile-power-cap", "215");
    await tick();
    // Props are still stale (210); the local overlay keeps 215 visible.
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("215");
    rerender({ activeSource: "adhoc", activeSettings: settings({ power_cap_w: 215 }) });
    await tick();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("215");
  });

  it("a committed value stays visible after activeSettings catches up (transient edit)", async () => {
    // With transient edits the SAVED preset never catches up to a committed
    // value — only the per-slot applied state does. The local overlay must
    // survive the catch-up, or the field snaps back to the saved value.
    const { rerender } = renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210, max_mclk_mhz: 1350 })],
      activeSettings: settings({ power_cap_w: 210, max_mclk_mhz: 1350 }),
    });
    await commitFieldInput("tile-mclk", "1320");
    await tick();
    expect(screen.getByTestId("tile-mclk")).toHaveValue("1320");
    rerender({ activeSettings: settings({ power_cap_w: 210, max_mclk_mhz: 1320 }) });
    await tick();
    expect(screen.getByTestId("tile-mclk")).toHaveValue("1320");
    expect(screen.getByTestId("tile-preset-save")).not.toBeDisabled();
  });

  it("a failed apply clears the overlay and reverts to the effective value", async () => {
    vi.mocked(apply).mockRejectedValueOnce(new Error("boom"));
    renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210 })],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    await commitFieldInput("tile-power-cap", "215");
    await tick();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("210");
    expect(screen.getByTestId("tile-power-cap").className).not.toContain("border-danger");
  });

  it("blurring an untouched field does not commit", async () => {
    renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210 })],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    const el = screen.getByTestId("tile-power-cap");
    await fireEvent.blur(el);
    await tick();
    expect(apply).not.toHaveBeenCalled();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("210");
  });

  it("committing a value on a user preset that fails to apply does not persist", async () => {
    vi.mocked(apply).mockResolvedValueOnce({ ts: 0, ok: false, per_gpu: {} });
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3")] });
    await commitFieldInput("tile-sclk", "100");
    await tick();
    expect(updatePreset).not.toHaveBeenCalled();
  });

  it("an external activeSettings change clears a committed overlay", async () => {
    const { rerender } = renderTile({
      activeSource: "preset:1",
      presets: [preset(1, "a")],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    await commitFieldInput("tile-power-cap", "215");
    await tick();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("215");
    rerender({ activeSource: "stock", activeSettings: null });
    await tick();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("300"); // stock fallback
  });

  it("mount: null/stock source selects default", () => {
    renderTile();
    expect(screen.getByTestId("tile-preset")).toHaveValue("stock");
    expect(screen.getByTestId("tile-preset")).toBeReadOnly();
  });

  it("mount: preset:<id> selects that preset", () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3")] });
    expect(screen.getByTestId("tile-preset")).toHaveValue("p3");
  });

  it("mount: adhoc source selects a custom state", () => {
    renderTile({ activeSource: "adhoc" });
    expect(screen.getByTestId("tile-preset")).toHaveValue("adhoc");
  });

  it("mount: dangling preset id selects the custom state", () => {
    renderTile({ activeSource: "preset:9" });
    expect(screen.getByTestId("tile-preset")).toHaveValue("custom");
  });

  const KNOBS_EXT = ["tile-power-cap", "tile-sclk", "tile-voltage", "tile-mclk"];

  it("external source shows the 'modified outside amd-gpu-tuned' badge", () => {
    renderTile({
      activeSource: "external",
      activeSettings: settings({ power_cap_w: 235, max_mclk_mhz: 1310 }),
    });
    expect(screen.getByTestId("tile-lamp-outside")).toBeVisible();
    expect(screen.getByText("Settings changed outside amd-gpu-tuned")).toHaveTextContent(
      "Settings changed outside amd-gpu-tuned",
    );
  });

  it("external source disables the knob fields (read-only base)", () => {
    renderTile({
      activeSource: "external",
      activeSettings: settings({ power_cap_w: 235, max_mclk_mhz: 1310 }),
    });
    for (const id of KNOBS_EXT) expect(screen.getByTestId(id)).toBeDisabled();
  });

  it("external source renders the captured values read-only", () => {
    renderTile({
      activeSource: "external",
      activeSettings: settings({ power_cap_w: 235, max_mclk_mhz: 1310 }),
    });
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("235");
    expect(screen.getByTestId("tile-mclk")).toHaveValue("1310");
  });

  it("external source: + (clone) is enabled, save is disabled", () => {
    renderTile({
      activeSource: "external",
      activeSettings: settings({ power_cap_w: 235, max_mclk_mhz: 1310 }),
    });
    expect(screen.getByTestId("tile-preset-add")).not.toBeDisabled();
    expect(screen.getByTestId("tile-preset-save")).toBeDisabled();
  });

  it("external source: preset selector shows no saved preset (label is the badge text)", () => {
    renderTile({
      activeSource: "external",
      activeSettings: settings({ power_cap_w: 235, max_mclk_mhz: 1310 }),
    });
    expect(screen.getByTestId("tile-preset")).toHaveValue("modified outside amd-gpu-tuned");
  });

  it("a non-external card shows no badge", () => {
    renderTile(); // stock
    // The external badge is not rendered at all (not just hidden): reserving
    // its 174px width permanently would overflow the header row and push the
    // throttle lamp past the card's content edge.
    expect(screen.queryByTestId("tile-lamp-outside")).toBeNull();
  });

  it("shows the drifted lamp while a restore is pending", () => {
    renderTile({ drifted: true });
    expect(screen.getByTestId("tile-lamp-drifted")).toBeVisible();
    expect(screen.getByText("Value drifted — auto-restoring")).toHaveTextContent(
      "Value drifted — auto-restoring",
    );
  });

  it("hides the drifted badge when stable", () => {
    renderTile({ drifted: false });
    expect(screen.queryByTestId("tile-lamp-drifted")).toBeNull();
  });

  it("shows the drifted badge independently of the external badge", () => {
    renderTile({ drifted: true, activeSource: "external" });
    expect(screen.getByTestId("tile-lamp-drifted")).toBeVisible();
    expect(screen.getByTestId("tile-lamp-outside")).toBeVisible();
  });

  it("adopts the active preset once the list loads after mount", async () => {
    // The tile mounts while the server is on preset 5, but the preset list has
    // not loaded yet (presets = []). It starts as "custom"; when the list
    // arrives it must adopt the active preset (not stay stuck on "custom").
    const { rerender } = renderTile({
      activeSource: "preset:5",
      presets: [],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    await tick();
    expect(screen.getByTestId("tile-preset")).toHaveValue("custom");
    rerender({ presets: [preset(5, "p5")] });
    await tick();
    expect(screen.getByTestId("tile-preset")).toHaveValue("p5");
  });

  const KNOBS = ["tile-power-cap", "tile-sclk", "tile-voltage", "tile-mclk"];

  it("disables the knob fields on stock (read-only base)", () => {
    renderTile(); // default props: activeSource null -> stock
    for (const id of KNOBS) expect(screen.getByTestId(id)).toBeDisabled();
  });

  it("disables the knob fields on a built-in preset", () => {
    renderTile({ activeSource: "preset:9", presets: [preset(9, "builtin", true)] });
    for (const id of KNOBS) expect(screen.getByTestId(id)).toBeDisabled();
  });

  it("enables the knob fields on a user preset", () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3")] });
    for (const id of KNOBS) expect(screen.getByTestId(id)).not.toBeDisabled();
  });

  it("a selected preset deleted elsewhere discards local state and re-derives", async () => {
    const { rerender } = renderTile({ activeSource: "preset:3", presets: [preset(3, "p3")] });
    await pickOption("tile-preset-opt-3");
    const combo = screen.getByTestId("tile-preset");
    await fireEvent.click(combo);
    await fireEvent.click(combo);
    await fireEvent.input(combo, { target: { value: "p3 x" } });
    await fireEvent.blur(combo);
    await tick();
    // name-only rename auto-persists; nothing is pending
    expect(updatePreset).toHaveBeenCalledWith(3, { name: "p3 x" });
    rerender({ presets: [] });
    await tick();
    expect(screen.getByTestId("tile-preset")).toHaveValue("custom");
  });

  it("selecting a preset applies immediately (no apply button)", async () => {
    renderTile({ presets: [preset(3, "p3")] });
    await pickOption("tile-preset-opt-3");
    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith({ gpu: "s", preset_id: 3 });
  });

  it("+ on stock creates 'stock (1)' and applies the new preset", async () => {
    renderTile(); // stock; createPreset mock returns id 42
    await fireEvent.click(screen.getByTestId("tile-preset-add"));
    await tick();
    expect(createPreset).toHaveBeenCalledTimes(1);
    expect(vi.mocked(createPreset).mock.calls[0]![0]).toMatchObject({ name: "stock (1)" });
    expect(apply).toHaveBeenCalledWith({ gpu: expect.any(String), preset_id: 42 });
  });

  it("+ increments the counter past existing clone names", async () => {
    renderTile({ presets: [preset(5, "stock (1)"), preset(6, "stock (2)")] });
    await fireEvent.click(screen.getByTestId("tile-preset-add"));
    await tick();
    expect(vi.mocked(createPreset).mock.calls[0]![0]).toMatchObject({ name: "stock (3)" });
  });

  it("+ is disabled while busy", () => {
    renderTile({ busy: true });
    expect(screen.getByTestId("tile-preset-add")).toBeDisabled();
  });

  it("built-in dropdown options render a real (built-in) sublabel span", async () => {
    renderTile({ presets: [preset(3, "p3", true), preset(4, "u4")] });
    await fireEvent.click(screen.getByTestId("tile-preset"));
    const bi = screen.getByTestId("tile-preset-opt-3");
    const span = bi.querySelector("span");
    expect(span?.textContent).toBe("(built-in)");
    expect(bi.textContent?.includes(" (built-in)")).toBe(true);
    expect(screen.getByTestId("tile-preset-opt-4").querySelector("span")).toBeNull();
  });

  it("selecting default reverts", async () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3", true)] });
    await pickOption("tile-preset-opt-default");
    expect(revert).toHaveBeenCalledTimes(1);
    expect(revert).toHaveBeenCalledWith({ gpu: "s" });
  });

  it("editing a field on a preset applies through the preset_id (label stays preset)", async () => {
    renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 220, voltage_offset_mv: -70, max_mclk_mhz: 1350 })],
      activeSettings: settings({ power_cap_w: 220, voltage_offset_mv: -70, max_mclk_mhz: 1350 }),
    });
    await pickOption("tile-preset-opt-3");
    await commitFieldInput("tile-sclk", "100");
    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenLastCalledWith({
      gpu: "s",
      preset_id: 3,
      overrides: settings({
        power_cap_w: 220,
        sclk_offset_mhz: 100,
        voltage_offset_mv: -70,
        max_mclk_mhz: 1350,
      }),
    });
  });

  it("remount shows the applied value, not the saved preset value (transient edit survives reload)", () => {
    // After a transient field edit the card runs the edited value (recorded in
    // the per-slot applied state) while the SAVED preset keeps the old value.
    // Closing and reopening the UI remounts the tile: it must show the value
    // the card is actually running, not the stale saved preset value.
    renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210 })],
      activeSettings: settings({ power_cap_w: 235 }),
    });
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("235");
  });

  it("an edited-but-not-reapplied preset does not change the shown (applied) value", async () => {
    const { rerender } = renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210 })],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("210");
    // The preset is edited to 218 (panel, or a Save on another tile that
    // shares it). Editing the saved preset does not touch this card — it is
    // still running the previously applied 210. The tile must show what the
    // card is running (the per-slot applied value), not the not-yet-applied
    // preset value.
    rerender({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 218 })],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    await tick();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("210");
  });

  it("two-click on a user preset enters rename mode", async () => {
    renderTile({ presets: [preset(3, "p3")] });
    await pickOption("tile-preset-opt-3");
    const combo = screen.getByTestId("tile-preset");
    await fireEvent.click(combo);
    await fireEvent.click(combo);
    expect(combo).toBeEditable();
  });

  it("two-click rename keeps the list open while editing; Enter commits and closes it", async () => {
    renderTile({ presets: [preset(3, "p3")] });
    await pickOption("tile-preset-opt-3");
    const combo = screen.getByTestId("tile-preset");
    await fireEvent.click(combo);
    expect(screen.getByTestId("tile-preset-list")).not.toBeNull();
    await fireEvent.click(combo);
    expect(combo).toBeEditable();
    expect(screen.getByTestId("tile-preset-list")).not.toBeNull();
    await fireEvent.input(combo, { target: { value: "p3 renamed" } });
    await fireEvent.keyDown(combo, { key: "Enter" });
    await tick();
    expect(updatePreset).toHaveBeenCalledWith(3, { name: "p3 renamed" });
    expect(screen.queryByTestId("tile-preset-list")).toBeNull();
  });

  it("two-click on a built-in does NOT enter rename mode", async () => {
    renderTile({ presets: [preset(3, "bi", true)] });
    await pickOption("tile-preset-opt-3");
    const combo = screen.getByTestId("tile-preset");
    await fireEvent.click(combo);
    await fireEvent.click(combo);
    expect(combo).toBeReadOnly();
  });

  it("two-click on default does NOT enter rename mode", async () => {
    renderTile();
    const combo = screen.getByTestId("tile-preset");
    await fireEvent.click(combo);
    await fireEvent.click(combo);
    expect(combo).toBeReadOnly();
  });

  it("renaming: Escape cancels and restores the original name", async () => {
    renderTile({ presets: [preset(3, "p3")] });
    await pickOption("tile-preset-opt-3");
    const combo = screen.getByTestId("tile-preset");
    await fireEvent.click(combo);
    await fireEvent.click(combo);
    await fireEvent.input(combo, { target: { value: "changed" } });
    await fireEvent.keyDown(combo, { key: "Escape" });
    await tick();
    expect(combo).toHaveValue("p3");
  });

  it("clicking the field while renaming is a no-op (list stays open)", async () => {
    renderTile({ presets: [preset(3, "p3")] });
    await pickOption("tile-preset-opt-3");
    const combo = screen.getByTestId("tile-preset");
    await fireEvent.click(combo);
    await fireEvent.click(combo);
    await fireEvent.click(combo);
    await tick();
    expect(screen.getByTestId("tile-preset-list")).not.toBeNull();
  });

  it("blocks an out-of-range field edit with an inline error and no apply", async () => {
    renderTile({ presets: [preset(1, "a")] });
    await pickOption("tile-preset-opt-1");
    await commitFieldInput("tile-sclk", "2000");
    expect(apply).toHaveBeenCalledTimes(1); // only the pick
    expect(screen.getByTestId("tile-sclk").className).toContain("border-danger");
    expect(toasts.some((t) => t.tone === "error")).toBe(false);
  });

  it("shows no range error while typing, only after Enter", async () => {
    renderTile({ presets: [preset(1, "a")] });
    await pickOption("tile-preset-opt-1");
    await fireEvent.input(screen.getByTestId("tile-sclk"), { target: { value: "2000" } });
    await tick();
    expect(screen.getByTestId("tile-sclk").className).not.toContain("border-danger");
    await fireEvent.keyDown(screen.getByTestId("tile-sclk"), { key: "Enter" });
    await tick();
    expect(screen.getByTestId("tile-sclk").className).toContain("border-danger");
    expect(apply).toHaveBeenCalledTimes(1); // only the pick, no field apply
  });

  it("clears the inline range error when Enter is pressed with a valid value", async () => {
    renderTile({ presets: [preset(1, "a")] });
    await pickOption("tile-preset-opt-1");
    await commitFieldInput("tile-sclk", "2000");
    expect(screen.getByTestId("tile-sclk").className).toContain("border-danger");
    await commitFieldInput("tile-sclk", "100");
    expect(screen.getByTestId("tile-sclk").className).not.toContain("border-danger");
  });

  it("does not apply on commit when the field is invalid", async () => {
    renderTile({ presets: [preset(1, "a")] });
    await pickOption("tile-preset-opt-1");
    await commitFieldInput("tile-sclk", "9999");
    expect(apply).toHaveBeenCalledTimes(1); // only the pick
  });

  it("blur without Enter commits a valid typed value", async () => {
    renderTile({ presets: [preset(1, "a")] });
    await pickOption("tile-preset-opt-1");
    await fireEvent.input(screen.getByTestId("tile-sclk"), { target: { value: "55" } });
    await fireEvent.blur(screen.getByTestId("tile-sclk"));
    await tick();
    expect(screen.getByTestId("tile-sclk")).toHaveValue("55");
    expect(apply).toHaveBeenCalledTimes(2); // the pick + the blur commit
  });

  it("blur after a failed Enter restores the previous value and clears the error", async () => {
    renderTile({ presets: [preset(1, "a")] });
    await pickOption("tile-preset-opt-1");
    await commitFieldInput("tile-sclk", "2000");
    expect(screen.getByTestId("tile-sclk").className).toContain("border-danger");
    await fireEvent.blur(screen.getByTestId("tile-sclk"));
    await tick();
    expect(screen.getByTestId("tile-sclk")).toHaveValue("0");
    expect(screen.getByTestId("tile-sclk").className).not.toContain("border-danger");
  });

  it("shows no error when the knob has no known range", async () => {
    const noRange = { ...RANGED, sclk_offset_range_mhz: null };
    render(GpuTile, {
      props: {
        sample: sample(),
        staticInfo: noRange,
        stale: false,
        presets: [preset(1, "a")],
        busy: false,
        activeSource: null,
        activeSettings: null,
        onPresetsChanged: () => {},
        onStatusChanged: () => {},
      },
    });
    await pickOption("tile-preset-opt-1");
    await commitFieldInput("tile-sclk", "99999");
    await tick();
    expect(screen.getByTestId("tile-sclk").className).not.toContain("border-danger");
  });

  it("shows the short tooltip over the whole field", () => {
    renderTile();
    expect(screen.getAllByRole("tooltip").length).toBe(4);
    expect(screen.getByText("Power cap (50 .. 350 W)")).toBeVisible();
  });

  it("editing a knob applies live but does not persist (transient)", async () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3", false, { power_cap_w: 220 })] });
    await commitFieldInput("tile-sclk", "100");
    await tick();
    expect(apply).toHaveBeenCalledTimes(1);
    expect(updatePreset).not.toHaveBeenCalled();
    expect(screen.getByTestId("tile-preset-save")).not.toBeDisabled();
  });

  it("blurring a typed value commits it live (no auto-persist)", async () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3")] });
    const el = screen.getByTestId("tile-power-cap");
    await fireEvent.input(el, { target: { value: "215" } });
    await fireEvent.blur(el);
    await tick();
    expect(apply).toHaveBeenCalledTimes(1);
    expect(updatePreset).not.toHaveBeenCalled();
  });

  it("Save persists the four knobs", async () => {
    renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 220 })],
      activeSettings: settings({ power_cap_w: 220 }),
    });
    await commitFieldInput("tile-sclk", "100");
    await tick();
    expect(updatePreset).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByTestId("tile-preset-save"));
    await tick();
    expect(updatePreset).toHaveBeenCalledWith(
      3,
      expect.objectContaining({
        settings: expect.objectContaining({
          sclk_offset_mhz: 100,
          power_cap_w: 220,
        }),
      }),
    );
    expect(screen.getByTestId("tile-preset-save")).toBeDisabled();
  });

  it("has no delete button (moved to the presets panel)", () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3")] });
    expect(screen.queryByTestId("tile-preset-delete")).toBeNull();
  });

  it("switching preset while modified opens the unsaved-changes dialog", async () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3"), preset(4, "p4")] });
    await commitFieldInput("tile-sclk", "100");
    await tick();
    await pickOption("tile-preset-opt-4");
    await tick();
    expect(screen.getByRole("dialog")).not.toBeNull();
    expect(apply).toHaveBeenCalledTimes(1); // only the commit; the switch is held
  });

  it("switch dialog: Stay cancels the switch", async () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3"), preset(4, "p4")] });
    await commitFieldInput("tile-sclk", "100");
    await tick();
    await pickOption("tile-preset-opt-4");
    await tick();
    await fireEvent.click(screen.getByTestId("tile-switch-stay"));
    await tick();
    expect(screen.getByTestId("tile-preset")).toHaveValue("p3");
    expect(apply).toHaveBeenCalledTimes(1);
  });

  it("switch dialog: Save persists then switches", async () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3"), preset(4, "p4")] });
    await commitFieldInput("tile-sclk", "100");
    await tick();
    await pickOption("tile-preset-opt-4");
    await tick();
    await fireEvent.click(screen.getByTestId("tile-switch-save"));
    await tick();
    expect(updatePreset).toHaveBeenCalledWith(
      3,
      expect.objectContaining({
        settings: expect.objectContaining({ sclk_offset_mhz: 100 }),
      }),
    );
    expect(apply).toHaveBeenLastCalledWith({ gpu: "s", preset_id: 4 });
  });

  it("switch dialog: Dismiss discards then switches (no persist)", async () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3"), preset(4, "p4")] });
    await commitFieldInput("tile-sclk", "100");
    await tick();
    await pickOption("tile-preset-opt-4");
    await tick();
    await fireEvent.click(screen.getByTestId("tile-switch-dismiss"));
    await tick();
    expect(updatePreset).not.toHaveBeenCalled();
    expect(apply).toHaveBeenCalledWith({ gpu: "s", preset_id: 3 }); // discard re-apply
    expect(apply).toHaveBeenLastCalledWith({ gpu: "s", preset_id: 4 }); // then switch
  });

  it("pick while modified: Dismiss clears the committed overlay and switches", async () => {
    // Rewrite of the old "pick clears a committed overlay": with the switch
    // guard, a pick while `modified` is held until the dialog is resolved.
    renderTile({ activeSource: "preset:1", presets: [preset(1, "a"), preset(3, "p3")] });
    await commitFieldInput("tile-power-cap", "215");
    await tick();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("215");
    await pickOption("tile-preset-opt-3");
    await tick();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("215"); // held, dialog open
    await fireEvent.click(screen.getByTestId("tile-switch-dismiss"));
    await tick();
    expect(screen.getByTestId("tile-power-cap")).toHaveValue("300");
  });

  it("registers unsaved changes while modified; clears after Save", async () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3")] });
    expect(hasPending()).toBe(false);
    await commitFieldInput("tile-sclk", "100");
    await tick();
    expect(hasPending()).toBe(true);
    await fireEvent.click(screen.getByTestId("tile-preset-save"));
    await tick();
    expect(hasPending()).toBe(false);
  });

  it("clears the pending registration after discard", async () => {
    renderTile({ activeSource: "preset:3", presets: [preset(3, "p3"), preset(4, "p4")] });
    await commitFieldInput("tile-sclk", "100");
    await tick();
    expect(hasPending()).toBe(true);
    await pickOption("tile-preset-opt-4");
    await tick();
    await fireEvent.click(screen.getByTestId("tile-switch-dismiss"));
    await tick();
    expect(hasPending()).toBe(false);
  });

  describe("server-source sync (no reload)", () => {
    it("external adoption is visible without a reload", async () => {
      const { rerender } = renderTile({
        activeSource: "preset:7",
        presets: [preset(7, "p7", false, { power_cap_w: 225, voltage_offset_mv: -70 })],
        activeSettings: settings({ power_cap_w: 225, voltage_offset_mv: -70 }),
      });
      expect(screen.getByTestId("tile-preset")).toHaveValue("p7");
      rerender({
        activeSource: "external",
        activeSettings: settings({ power_cap_w: 210, voltage_offset_mv: -70 }),
      });
      await tick();
      expect(screen.getByTestId("tile-preset")).toHaveValue("modified outside amd-gpu-tuned");
      expect(screen.getByTestId("tile-lamp-outside")).toBeVisible();
    });

    it("stock revert is visible without a reload", async () => {
      const { rerender } = renderTile({
        activeSource: "preset:7",
        presets: [preset(7, "p7", false, { power_cap_w: 225 })],
        activeSettings: settings({ power_cap_w: 225 }),
      });
      rerender({ activeSource: "stock", activeSettings: null });
      await tick();
      expect(screen.getByTestId("tile-preset")).toHaveValue("stock");
    });

    it("a second client's preset apply is visible without a reload", async () => {
      const { rerender } = renderTile({
        activeSource: "preset:7",
        presets: [preset(7, "p7", false, { power_cap_w: 225 }), preset(15, "p15", false, { power_cap_w: 230 })],
        activeSettings: settings({ power_cap_w: 225 }),
      });
      rerender({ activeSource: "preset:15", activeSettings: settings({ power_cap_w: 230 }) });
      await tick();
      expect(screen.getByTestId("tile-preset")).toHaveValue("p15");
    });

    it("an in-flight pick is not clobbered by a stale poll", async () => {
      // Type the promise as ApplyReport (the real `apply` return type) — a
      // narrower object literal type fails `npm run check` (missing `ts`).
      let resolveApply: (v: ApplyReport) => void = () => {};
      vi.mocked(apply).mockImplementationOnce(
        () => new Promise<ApplyReport>((resolve) => { resolveApply = resolve; }),
      );
      const { rerender } = renderTile({
        activeSource: "external",
        presets: [preset(7, "p7", false, { power_cap_w: 225 })],
        activeSettings: settings({ power_cap_w: 210 }),
      });
      await pickOption("tile-preset-opt-7");
      await tick();
      expect(screen.getByTestId("tile-preset")).toHaveValue("p7"); // optimistic
      // Stale poll while the apply is in flight: selection must be kept.
      rerender({ activeSource: "external", activeSettings: settings({ power_cap_w: 210 }) });
      await tick();
      expect(screen.getByTestId("tile-preset")).toHaveValue("p7");
      resolveApply({ ts: 0, ok: true, per_gpu: {} });
      rerender({ activeSource: "preset:7", activeSettings: settings({ power_cap_w: 225 }) });
      await tick();
      expect(screen.getByTestId("tile-preset")).toHaveValue("p7"); // landed, confirmed, no-op
    });

    it("unsaved edits are discarded when the source moves away from the preset", async () => {
      const { rerender } = renderTile({
        activeSource: "preset:7",
        presets: [preset(7, "p7", false, { power_cap_w: 225 })],
        activeSettings: settings({ power_cap_w: 225 }),
      });
      await commitFieldInput("tile-power-cap", "220");
      await tick();
      expect(screen.getByTestId("tile-preset-save")).not.toBeDisabled(); // modified
      rerender({ activeSource: "external", activeSettings: settings({ power_cap_w: 210 }) });
      await tick();
      expect(screen.getByTestId("tile-preset")).toHaveValue("modified outside amd-gpu-tuned");
      expect(screen.getByTestId("tile-preset-save")).toBeDisabled();
    });

    it("a failed apply snaps the label back to the server source", async () => {
      // The mock resolves on a microtask, so by the first `tick()` the apply
      // has already failed and `saving` is cleared — the snap-back is
      // observable directly (the optimistic in-flight state is covered by the
      // in-flight test above, which holds the promise open).
      vi.mocked(apply).mockResolvedValueOnce({ ts: 0, ok: false, per_gpu: {} });
      renderTile({
        activeSource: "external",
        presets: [preset(7, "p7", false, { power_cap_w: 225 })],
        activeSettings: settings({ power_cap_w: 210 }),
      });
      await pickOption("tile-preset-opt-7");
      await tick();
      expect(screen.getByTestId("tile-preset")).toHaveValue("modified outside amd-gpu-tuned");
    });

    it("a Save of the shown preset does not pin the label against a later external change", async () => {
      const { rerender } = renderTile({
        activeSource: "preset:7",
        presets: [preset(7, "p7", false, { power_cap_w: 225 })],
        activeSettings: settings({ power_cap_w: 225 }),
      });
      await commitFieldInput("tile-power-cap", "220");
      await tick();
      expect(screen.getByTestId("tile-preset-save")).not.toBeDisabled();
      await fireEvent.click(screen.getByTestId("tile-preset-save"));
      await tick();
      // savedPreset is now set; the preset IS in the list. A later external
      // change must still downgrade the label (guards the narrow
      // savedPreset condition).
      rerender({ activeSource: "external", activeSettings: settings({ power_cap_w: 210 }) });
      await tick();
      expect(screen.getByTestId("tile-preset")).toHaveValue("modified outside amd-gpu-tuned");
    });
  });

  it("marks the diverged knob with a dot and amber border, and not the others", async () => {
    renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210, max_mclk_mhz: 1350 })],
      activeSettings: settings({ power_cap_w: 210, max_mclk_mhz: 1350 }),
    });
    await commitFieldInput("tile-power-cap", "215");
    await tick();
    expect(screen.getByTestId("tile-diverged-power-cap")).toBeVisible();
    expect(screen.getByTestId("tile-power-cap").className).toContain("border-warn");
    expect(screen.queryByTestId("tile-diverged-mclk")).toBeNull();
    expect(screen.getByTestId("tile-mclk").className).not.toContain("border-warn");
  });

  it("diverged dot is absent for stock", () => {
    renderTile();
    expect(screen.queryByTestId("tile-diverged-power-cap")).toBeNull();
    expect(screen.queryByTestId("tile-diverged-mclk")).toBeNull();
  });

  it("diverged dot clears once the saved preset matches the committed value", async () => {
    const { rerender } = renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210 })],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    await commitFieldInput("tile-power-cap", "215");
    await tick();
    expect(screen.getByTestId("tile-diverged-power-cap")).toBeVisible();
    // The preset is saved to 215 (now equal to the committed local value).
    rerender({
      presets: [preset(3, "p3", false, { power_cap_w: 215 })],
      activeSettings: settings({ power_cap_w: 215 }),
    });
    await tick();
    expect(screen.queryByTestId("tile-diverged-power-cap")).toBeNull();
  });

  it("range error wins over the diverged border tint", async () => {
    renderTile({
      activeSource: "preset:3",
      presets: [preset(3, "p3", false, { power_cap_w: 210 })],
      activeSettings: settings({ power_cap_w: 210 }),
    });
    // 5 is out of the power-cap range (cap_min_w = 50) → commitField sets
    // fieldError and returns before writing `local`, so the input is in the
    // error state; the amber diverged tint must not also be applied.
    await commitFieldInput("tile-power-cap", "5");
    await tick();
    expect(screen.getByTestId("tile-power-cap").className).toContain("border-danger");
    expect(screen.getByTestId("tile-power-cap").className).not.toContain("border-warn");
  });
});

describe("ECC row", () => {
  it("renders ECC — faint when all ecc counts are null", () => {
    const { container } = renderTile();
    const el = screen.getByTestId("tile-ecc");
    expect(el.textContent).toContain("ECC —");
    expect(container.querySelector('[data-testid="tile-ecc"] .text-warn')).toBeNull();
    expect(container.querySelector('[data-testid="tile-ecc"] .text-danger')).toBeNull();
  });

  it("colors UE red when uncorrectable > 0", () => {
    renderTile({ sample: sample({ ecc_umc_ce: 0, ecc_umc_ue: 2, ecc_umc_deferred: 0 }) });
    const el = screen.getByTestId("tile-ecc");
    expect(el.textContent).toContain("2u");
    expect(el.querySelector(".text-danger")).not.toBeNull();
  });

  it("shows c/u/d letter suffixes on the counts", () => {
    renderTile({ sample: sample({ ecc_umc_ce: 3, ecc_umc_ue: 0, ecc_umc_deferred: 2 }) });
    const el = screen.getByTestId("tile-ecc");
    expect(el.textContent).toContain("3c");
    expect(el.textContent).toContain("0u");
    expect(el.textContent).toContain("2d");
  });

  it("carries a title tooltip with the full counter names", () => {
    renderTile({ sample: sample({ ecc_umc_ce: 3, ecc_umc_ue: 1, ecc_umc_deferred: 2 }) });
    const el = screen.getByTestId("tile-ecc");
    const eccCell = el.firstElementChild as HTMLElement;
    expect(eccCell.title).toContain("correctable: 3");
    expect(eccCell.title).toContain("uncorrectable: 1");
    expect(eccCell.title).toContain("deferred: 2");
  });

  it("colors CE amber when new correctables appear in the window", () => {
    const ring = [
      sample({ ecc_umc_ce: 3, ecc_umc_ue: 0, ecc_umc_deferred: 0, ts_ms: 1000 }),
      sample({ ecc_umc_ce: 5, ecc_umc_ue: 0, ecc_umc_deferred: 0, ts_ms: 2000 }),
    ];
    renderTile({ sample: ring[1]!, ring });
    const el = screen.getByTestId("tile-ecc");
    expect(el.querySelector(".text-warn")).not.toBeNull();
  });

  it("keeps CE green when stable in the window", () => {
    const ring = [
      sample({ ecc_umc_ce: 5, ecc_umc_ue: 0, ecc_umc_deferred: 0, ts_ms: 1000 }),
      sample({ ecc_umc_ce: 5, ecc_umc_ue: 0, ecc_umc_deferred: 0, ts_ms: 2000 }),
    ];
    renderTile({ sample: ring[1]!, ring });
    const el = screen.getByTestId("tile-ecc");
    expect(el.querySelector(".text-warn")).toBeNull();
    expect(el.querySelector(".text-success")).not.toBeNull();
  });
});

describe("PCIe cell", () => {
  it("renders the link state with the ok band", () => {
    renderTile({ sample: sample({ pcie_width: 16, pcie_speed_gt: 8.0 }) });
    const el = screen.getByTestId("tile-ecc");
    expect(el.textContent).toContain("PCIe");
    expect(el.textContent).toContain("x16 · 8.0 GT/s");
    expect(el.querySelector(".text-success")).not.toBeNull();
  });

  it("colors the link state amber when degraded", () => {
    renderTile({ sample: sample({ pcie_width: 8, pcie_speed_gt: 8.0 }) });
    const el = screen.getByTestId("tile-ecc");
    expect(el.querySelector(".text-warn")).not.toBeNull();
  });

  it("renders PCIe — faint when data is null", () => {
    renderTile();
    const el = screen.getByTestId("tile-ecc");
    expect(el.textContent).toContain("PCIe —");
  });
});
