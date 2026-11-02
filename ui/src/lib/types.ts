// TS mirrors of the daemon API shapes (src/types.rs + handler JSON).
// Sparse fields arrive as JSON `null` or are absent — always code with `?? null`.

export interface GpuSample {
  slot: string;
  ts_ms: number;
  socket_power_w: number;
  power_cap_w: number | null;
  edge_c: number | null;
  hotspot_c: number | null;
  vram_c: number | null;
  gfx_mhz: number | null;
  mem_mhz: number | null;
  gfx_activity_pct: number | null;
  mem_activity_pct: number | null;
  vram_used_mb: number | null;
  vram_total_mb: number | null;
  dpm_sclk_mhz: number | null;
  dpm_mclk_mhz: number | null;
  throttle_bits: string[] | null;
  fan_rpm: number | null;
  fan_pwm_pct: number | null;
  voltage_gfx_mv: number | null;
  voltage_soc_mv: number | null;
  voltage_mem_mv: number | null;
  ecc_umc_ce: number | null;
  ecc_umc_ue: number | null;
  ecc_umc_deferred: number | null;
  pcie_width: number | null;
  pcie_speed_gt: number | null;
}

export interface GpuStatic {
  slot: string;
  card_n: number;
  vbios: string | null;
  slowdown_edge_c: number | null;
  slowdown_hotspot_c: number | null;
  slowdown_vram_c: number | null;
  cap_min_w: number | null;
  cap_max_w: number | null;
  cap_default_w: number | null;
  dpm_sclk_levels: number[];
  dpm_mclk_levels: number[];
  overdrive: boolean;
  sclk_offset_range_mhz: [number, number] | null;
  mclk_range_mhz: [number, number] | null;
  voltage_offset_range_mv: [number, number] | null;
  pmfw_fan: boolean;
  thermal_target: boolean;
  power_profile: boolean;
}

export interface PresetSettings {
  power_cap_w: number | null;
  sclk_offset_mhz: number | null;
  max_mclk_mhz: number | null;
  voltage_offset_mv: number | null;
}

export interface Preset {
  id: number | null;
  name: string;
  notes: string;
  settings: PresetSettings;
  is_builtin: boolean;
  created_at: number | null;
  updated_at: number | null;
}

export interface GpuStatusEntry {
  slot: string;
  static: GpuStatic;
  live: { power_cap_w: number | null; od: unknown; fan: unknown };
  /** A drift episode is open on this card (auto-restore pending). */
  drifted?: boolean;
}

export interface Status {
  ok: boolean;
  version: string;
  uptime_ms: number;
  busy: boolean;
  /** Daemon can write the card (running as root). False ⇒ all tuning writes are blocked. */
  privileged: boolean;
  /** A write password is stored; writes require an unlock token. */
  auth_enabled: boolean;
  baseline_ts: number | null;
  baseline_age_ms: number | null;
  active_state: { source: string; settings: Record<string, PresetSettings> } | null;
  active_sources: Record<string, string> | null;
  gpus: GpuStatusEntry[];
}

export interface ApplyOutcome {
  field: string;
  status: "verified" | "applied" | "failed" | "skipped";
  detail: string;
}

export interface ApplyReport {
  ts: number;
  ok: boolean;
  per_gpu: Record<string, ApplyOutcome[]>;
}

export interface GpuSetup {
  slot: string;
  overdrive_table: boolean;
  pmfw_fan: boolean;
  hwmon: boolean;
  dpm_levels: boolean;
  thermal_target: boolean;
  power_profile: boolean;
}

export interface SetupCheck {
  ready: boolean;
  live_mask: number | null;
  overdrive_bit: boolean;
  gpus: GpuSetup[];
}

export interface RingMinutes {
  minutes: number;
}

export interface DriftKnobs {
  power_cap_w: number | null;
  sclk_offset_mhz: number | null;
  max_mclk_mhz: number | null;
  voltage_offset_mv: number | null;
}

export interface DriftEntry {
  id: number;
  ts: number;
  slot: string;
  detected: DriftKnobs;
  intended: DriftKnobs | null;
  previous_source: string;
  outcome: "pending" | "reapplied" | "reapply_failed" | "cleared" | "logged_only" | "adopted";
  outcome_detail: unknown;
}

export interface DriftPending {
  entry_id: number;
  deadline_ms: number;
}

export interface DriftStatus {
  reapply: boolean;
  delay_s: number;
  entries: DriftEntry[];
  pending: Record<string, DriftPending>;
}

export interface DriftSetup {
  reapply: boolean;
  delay_s: number;
}
