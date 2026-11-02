#!/usr/bin/env bash
# Regenerate sysfs fixtures when the kernel changes.
# Usage: capture_fixtures.sh [DRM_ROOT] [DEST]
set -euo pipefail
ROOT=${1:-/sys/class/drm}
DEST=${2:-fixtures/sysfs}
rm -rf "$DEST" && mkdir -p "$DEST"
for card in "$ROOT"/card[0-9]*; do
  name=$(basename "$card")
  mkdir -p "$DEST/$name/device"
  cp "$card/uevent" "$DEST/$name/" 2>/dev/null || true
  cp "$card/dev" "$DEST/$name/" 2>/dev/null || true
  cp "$card/device/uevent" "$DEST/$name/device/" 2>/dev/null || true
  if [ -e "$card/device/hwmon" ]; then
    cp -rL "$card/device/hwmon" "$DEST/$name/device/"
  fi
  for f in pp_dpm_sclk pp_dpm_mclk pp_od_clk_voltage pp_features \
           pp_power_profile_mode gpu_busy_percent vbios_version \
           power_dpm_force_performance_level; do
    cp "$card/device/$f" "$DEST/$name/device/" 2>/dev/null || true
  done
  cp -r "$card/device/gpu_od" "$DEST/$name/device/" 2>/dev/null || true
  cp "$card/device/mem_info_vram_used" "$DEST/$name/device/" 2>/dev/null || true
  cp "$card/device/mem_info_vram_total" "$DEST/$name/device/" 2>/dev/null || true
done
echo "fixtures written to $DEST"
