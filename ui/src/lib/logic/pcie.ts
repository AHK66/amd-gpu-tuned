import type { Band } from "./thermals";

// The rig's normal negotiated state (R9700 in a Gen3 x16 slot).
export const EXPECTED_PCIE_WIDTH = 16;
export const EXPECTED_PCIE_SPEED_GT = 8.0;

export function pcieBand(width: number | null, speed: number | null): Band {
  if (width != null && width < EXPECTED_PCIE_WIDTH) return "warn";
  if (speed != null && speed < EXPECTED_PCIE_SPEED_GT) return "warn";
  return "ok";
}
