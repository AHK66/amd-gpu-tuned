import type {
  ApplyReport,
  DriftSetup,
  DriftStatus,
  Preset,
  PresetSettings,
  RingMinutes,
  SetupCheck,
  Status,
} from "./types";
import { closeModal, openModal } from "./store/modal.svelte";
import { authState } from "./store/auth.svelte";

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

// In-memory only: lost on page reload by construction (spec §6.1).
let token: string | null = null;

export function clearToken(): void {
  token = null;
}

async function req<T>(method: string, url: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (token !== null) headers["authorization"] = `Bearer ${token}`;
  const res = await fetch(url, {
    method,
    headers: Object.keys(headers).length > 0 ? headers : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) {
    if (res.status === 401) {
      token = null;
      authState.unlocked = false;
      openModal("unlock");
    }
    let msg = res.statusText;
    try {
      const v = (await res.json()) as { error?: string };
      if (v.error) msg = v.error;
    } catch {
      // non-JSON error body — keep statusText
    }
    throw new ApiError(res.status, msg);
  }
  // DELETE returns 204 No Content with an empty body; res.json() throws on an
  // empty body, so short-circuit the no-body case before parsing.
  if (res.status === 204) return null as T;
  return (await res.json()) as T;
}

export const unlock = async (password: string) => {
  const r = await req<{ token: string; ttl_s: number }>("POST", "/api/auth/unlock", {
    password,
  });
  token = r.token;
  authState.unlocked = true;
  closeModal();
  return r;
};

export const lockNow = async () => {
  const r = await req<{ locked: boolean }>("POST", "/api/auth/lock", {});
  token = null;
  authState.unlocked = false;
  return r;
};

export const getStatus = () => req<Status>("GET", "/api/status");

export const listPresets = () => req<Preset[]>("GET", "/api/presets");
export const createPreset = (b: {
  name: string;
  notes?: string;
  settings: PresetSettings;
}) => req<Preset>("POST", "/api/presets", b);
export const updatePreset = (
  id: number,
  b: { name?: string; notes?: string; settings?: PresetSettings },
) => req<Preset>("PUT", `/api/presets/${id}`, b);
export const deletePreset = (id: number) => req<null>("DELETE", `/api/presets/${id}`);
export const exportPreset = (id: number) => req<Preset>("GET", `/api/presets/${id}/export`);
export const importPreset = (p: Preset) => req<Preset>("POST", "/api/presets/import", p);

export const apply = (b: {
  preset_id?: number;
  gpu: string;
  overrides?: PresetSettings;
}) => req<ApplyReport>("POST", "/api/apply", b);
export const revert = (b?: { gpu?: string }) => req<ApplyReport>("POST", "/api/revert", b ?? {});

export const getSetup = () => req<SetupCheck>("GET", "/api/setup");
export const verifySetup = () => req<SetupCheck>("POST", "/api/setup/verify", {});
export const getRingMinutes = () => req<RingMinutes>("GET", "/api/setup/ring");
export const setRingMinutes = (minutes: number) =>
  req<RingMinutes>("POST", "/api/setup/ring", { minutes });

export interface TelemetrySnapshot {
  latest: Record<string, import("./types").GpuSample>;
  ring: import("./types").GpuSample[];
}

export const getTelemetry = () => req<TelemetrySnapshot>("GET", "/api/telemetry");

export const getDrift = () => req<DriftStatus>("GET", "/api/drift");
export const clearDrift = () => req<{ cleared: number }>("DELETE", "/api/drift");
export const getDriftSetup = () => req<DriftSetup>("GET", "/api/setup/drift");
export const setDriftSetup = (b: { reapply: boolean; delay_s: number }) =>
  req<DriftSetup>("POST", "/api/setup/drift", b);
