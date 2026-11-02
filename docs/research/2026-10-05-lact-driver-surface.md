# LACT → amdgpu driver surface: reference for a native R9700 (gfx11/RDNA4) daemon

Researched from LACT `master` (v0.10.1, `/home/arne/projects/LACT`) and the `amdgpu-sysfs`
crate v0.22.0 (the sysfs layer LACT extracted; source obtained from crates.io, local copy in
`/tmp/opencode/lact/amdgpu-sysfs-0.22.0`).

## 0. Architecture (read this first)

- `lact-daemon/src/server/gpu_controller/amd.rs` (1830 lines) — the AMD controller: applies
  config, drives fan tasks, gathers stats.
- The **entire sysfs read/write layer is the external crate `amdgpu-sysfs` 0.22.0**
  (workspace dep, `Cargo.toml:16`). `GpuHandle` = one `/sys/class/drm/cardN/device` dir
  (`gpu_handle/mod.rs:36-81`); `HwMon` = one `device/hwmon/hwmonX` symlink target
  (`hw_mon.rs`).
- **There is NO VBIOS binary parsing in current LACT.** All "valid range" discovery comes from
  the driver's *human-readable* sysfs (the `OD_RANGE` block of `pp_od_clk_voltage`,
  `pp_dpm_sclk`/`pp_dpm_mclk`, `power1_cap_min/max`). The old LACT VBIOS parser (`pp_table`
  binary) is gone from the tree (grep for `pp_table` in the daemon source: zero hits; the
  kernel sysfs `pp_table` binary is only present in test fixtures, never parsed).
- Extra kernel interfaces used:
  - **DRM ioctls** via dynamically loaded `libdrm_amdgpu` (`libdrm_amdgpu_sys` crate):
    `AMDGPU_INFO` (device info, VBIOS version/date, VRAM GTT info), `AMDGPU_SENSOR_INFO`
    (`GFX_MCLK` → current memory clock), `AMDGPU_HW_IP_INFO` — all **read-only info**,
    no state writes. `lact-daemon/src/server/gpu_controller/amd/drm/amdgpu.rs:36-155`.
  - **debugfs**: `amdgpu_vbios` binary dump (`amd.rs:677-688, 1123-1126`),
    `<connector>/link_settings` for DP link info (`amd.rs:1462-1500`).
  - **DBus**: logind `PrepareForSleep` (`suspend.rs`), polkit auth, power-profiles-daemon
    (disables its `amdgpu_dpm` action, `system/power_profiles_daemon.rs`).
  - **netlink** `KOBJECT_UEVENT` (SUBSYSTEM=drm) for hotplug/driver-reload detection
    (`system.rs:273-323`, `lib.rs:126-140`).
  - **`/proc/<pid>/fdinfo`** for per-process VRAM + per-engine (gfx/dec/enc) utilization
    (`common/fdinfo.rs`, `amd.rs:58-65, 1448-1456`).
  - **KFD sysfs** (`/sys/class/kfd/kfd/topology/nodes/*`) for cache info only (`amd.rs:691-726`).

### Device identity / cardN → hwmonX mapping

- Enumeration: iterate `/sys/class/drm`, accept names matching `card*` without `-`
  (`handler.rs:1434`), use `cardN/device` as sysfs root (`handler.rs:1436`).
- GPU id for config storage: `VEN:DEV-SUBSYSVEN:SUBSYSDEV-PCI_SLOT`
  (`gpu_controller.rs:114-126`, e.g. `1002:75b4-1002:0000-0000:7e:00.0`). This is stable
  across cardN renumbering — use the same, not the cardN index.
- hwmon mapping: `GpuHandle::new_from_path` reads the `device/hwmon/` **symlink directory**
  the kernel creates (`gpu_handle/mod.rs:53-58`). LACT always uses
  `hw_monitors.first()` (`amd.rs:112-114, 496-501`) — no label matching, fine for a single
  hwmon per GPU.
- renderD minor resolution: `/dev/dri/by-path/pci-<slot>-card` → `renderD128` for the ioctl
  handle (`gpu_controller.rs:162-163`, `drm/amdgpu.rs:40-51`).

## 1. WRITE surface (every file LACT writes to change GPU state)

Base path: `D = /sys/class/drm/cardN/device`, `H = D/hwmon/hwmonX` (symlink).

| # | Node | Controls | Exact text written | Units | Code |
|---|------|----------|--------------------|-------|------|
| 1 | `D/pp_od_clk_voltage` | sclk/mclk limits, sclk offset, VDDC curve, VDDGFX voltage offset; reset via `r` | see §1.1 below | MHz / mV | `gpu_handle/mod.rs:303-337` (read/reset), `overdrive/rdna.rs:37-111` (write commands), `overdrive/gcn.rs:24-43` (old format) |
| 2 | `D/power_dpm_force_performance_level` | DPM mode gate. Must be `manual` before `pp_od_*`/`pp_dpm_*`/`pp_power_profile_mode` work | `auto`, `manual`, `high`, `low`, `profile_*` (`mod.rs:811-830`) | — | `mod.rs:196-198`; dance in `amd.rs:655-675, 1381-1387` |
| 3 | `D/pp_dpm_sclk` | enable/disable individual SCLK p-states (only in `manual`) | space-separated indices: `0 1 2 ` (`mod.rs:283-301`) | level index | `mod.rs:283-301` |
| 4 | `D/pp_dpm_mclk` | same for MCLK | same format | level index | `mod.rs:283-301` |
| 5 | `D/pp_dpm_socclk` / `pp_dpm_fclk` / `pp_dpm_dcefclk` / `pp_dpm_pcie` | same mechanism, other clocks (kind→filename map `power_levels.rs:71-81`) | same format | level index | `mod.rs:283-301` (LACT config only exposes core+memory, `amd.rs:1086-1090`) |
| 6 | `D/pp_power_profile_mode` | select predefined profile or override heuristics of the "custom" profile (requires `manual`) | `{index}\n` or per component `{index} {comp} {v1} {v2} - …\n` (`-` = unset) (`mod.rs:377-403`) | per-profile units | `mod.rs:349-404`; `amd.rs:1182-1199` |
| 7 | `H/power1_cap` | **socket power cap** | `{W × 1 000 000}\n` i.e. **microwatts** (`hw_mon.rs:110-113`) | µW | `hw_mon.rs:110-113`; clamping in `amd.rs:1343-1367` |
| 8 | `H/pwm1` | static fan duty (pre-RDNA3 fallback only) | `{0..255}\n` (`hw_mon.rs:149-151`) | 0-255 | `amd.rs:176-181` |
| 9 | `H/pwm1_enable` | fan control mode | `1` = manual, `2` = auto (`hw_mon.rs:237-244, 199-202`) | enum | `amd.rs:173-174, 253-255, 378-380, 415-418` |
| 10 | `H/fan1_target` | fan target RPM (implemented in crate, **never called** by the controller) | `{rpm}\n` | rpm | `hw_mon.rs:177-181` (dead code path) |
| 11 | `D/gpu_od/fan_ctrl/fan_curve` | PMFW fan curve (RDNA3+; the only fan interface on the R9700-class) | one line per point `{i} {temp} {speed}\n` then commit `c\n` (`mod.rs:691-717`) | °C, % | `amd.rs:123-163, 196-213` |
| 12 | `D/gpu_od/fan_ctrl/fan_target_temperature` | PMFW target temp | `{value}\n` + commit `c` | °C | `mod.rs:552-559`; `amd.rs:1266-1279` |
| 13 | `D/gpu_od/fan_ctrl/fan_minimum_pwm` | PMFW min fan % | `{value}\n` + `c` | % | `mod.rs:564-566`; `amd.rs:1281-1294` |
| 14 | `D/gpu_od/fan_ctrl/fan_zero_rpm_enable` | 0-RPM (silent) mode | `0`/`1` + `c` | bool | `mod.rs:571-578`; `amd.rs:1304-1321, 124-133` |
| 15 | `D/gpu_od/fan_ctrl/fan_zero_rpm_stop_temperature` | 0-RPM stop temp | `{value}\n` + `c` | °C | `mod.rs:583-590`; `amd.rs:1323-1341` |
| 16 | `D/gpu_od/fan_ctrl/acoustic_limit_rpm_threshold` | fan acoustic ceiling | `{rpm}\n` + `c` | rpm | `mod.rs:526-533`; `amd.rs:1236-1249` |
| 17 | `D/gpu_od/fan_ctrl/acoustic_target_rpm_threshold` | fan acoustic target | `{rpm}\n` + `c` | rpm | `mod.rs:539-546`; `amd.rs:1251-1264` |
| 18 | `D/uma/carveout` | iGPU VRAM carveout (not relevant for R9700) | `{index}\n` | — | `mod.rs:744-746` |

Non-sysfs writes: `/etc/modprobe.d/99-amdgpu-overdrive.conf`
(`options amdgpu ppfeaturemask=0x{mask:X}`, `system.rs:92-105`) — see §4.

### 1.1 `pp_od_clk_voltage` multi-line protocol (the core)

The file accepts a sequence of single-line commands; many ASICs **stage** them until a
commit line `c` is written (kernel behavior differs: some ASICs commit each line, e.g.
sienna_cichlid — `gpu_handle/mod.rs:322-329` cites both kernel paths). LACT therefore
always ends with `c\n` via `CommitHandle::commit` (`mod.rs:844-858`).

- Reset: write `r\n` (`mod.rs:331-337`).
- Format auto-detection from the file content: "RDNA" format if it contains
  `VDDC_CURVE` or `OD_VDDGFX_OFFSET` or starts `OD_SCLK:` with `MHz` values, else GCN
  (`overdrive/mod.rs:176-193`).

**RDNA (Vega20+) command lines** (`overdrive/rdna.rs`):

| Command | Format string | Meaning | Units | Code |
|---|---|---|---|---|
| sclk offset (RDNA4+ only) | `format!("s {sclk_offset}\n")` — one value | shift of the whole sclk table | MHz | `rdna.rs:80-85, 456-458` (symbol `s`, index omitted) |
| sclk min/max | `format!("{symbol} {index} {clockspeed}\n")` → `s 0 {min}`, `s 1 {max}` | clamp sclk range | MHz | `rdna.rs:61-64, 87-94, 456-458` |
| mclk min/max | `m 0 {min}`, `m 1 {max}` | clamp mclk range | MHz | `rdna.rs:75-78, 87-94` |
| VDDC curve point | `format!("vc {index} {clockspeed} {voltage}\n")` | override FV curve point i | MHz, mV | `rdna.rs:96-101, 460-462` |
| VDDGFX offset (undervolt, RDNA3+) | `format!("vo {offset}\n")` | voltage offset on target-V calculation | mV | `rdna.rs:103-108, 464-466` |

**GCN (Vega10 and older) command lines** (`overdrive/gcn.rs:260-266`):
`format!("{symbol} {i} {clockspeed} {voltage}\n")` → `s {i} {MHz} {mV}` / `m {i} {MHz} {mV}`
(clock and voltage per level).

There is **no fan field** in the current `pp_od_clk_voltage` — LACT never writes fan via
this file. Fan is hwmon `pwm1` (old ASICs) or `gpu_od/fan_ctrl/*` (RDNA3+).

**Write order for RDNA** (`rdna.rs:47-108`), with quirk:
1. If new sclk min > previous sclk max → write `s 1 {new max}` *first* ("avoid an error on
   RDNA3", `rdna.rs:50-59`); same for mclk (`rdna.rs:66-73`).
2. `s 0 {min}`, `s 1 {max}` (only the ones set — see `clear()` below).
3. `m 0 {min}`, `m 1 {max}`.
4. (RDNA4) `s {sclk_offset}` — note: this line is actually emitted *before* the s/m lines
   in the code (`rdna.rs:80-85` runs before the loop at 87-94).
5. `vc {i} …` for every curve point.
6. `vo {offset}`.
7. (later, by caller) `c`.

Before writing, LACT calls `table.clear()` (drop all current values, keep ranges —
`rdna.rs:351-356`) and `table.normalize_vddc_curve()` (clamp curve points into allowed
range, `rdna.rs:358-372`) because "some GPUs have default values outside of the allowed
range" (`amd.rs:1511-1517`, cites amdgpu-clocks#32).

### 1.2 End-to-end apply sequence (from the golden test snapshot)

`lact-daemon/src/tests/snapshots/lact_daemon__tests__apply_config__amd__rx9070xt__config.yaml.snap`
(RDNA4-style card) shows the exact write order for a config with sclk offset +200 MHz,
voltage offset −50 mV, a 5-point fan curve and a 280 W cap:

```
card1/device/power_dpm_force_performance_level: auto
card1/device/power_dpm_force_performance_level: manual
card1/device/pp_od_clk_voltage: r
card1/device/pp_od_clk_voltage: s 200
card1/device/pp_od_clk_voltage: vo -50
card1/device/gpu_od/fan_ctrl/fan_curve: 0 40 35
card1/device/gpu_od/fan_ctrl/fan_curve: 1 50 40
card1/device/gpu_od/fan_ctrl/fan_curve: 2 60 50
card1/device/gpu_od/fan_ctrl/fan_curve: 3 80 80
card1/device/gpu_od/fan_ctrl/fan_curve: 4 90 100
card1/device/hwmon/hwmon5/power1_cap: 280000000
card1/device/gpu_od/fan_ctrl/fan_curve: c
card1/device/pp_od_clk_voltage: c
card1/device/power_dpm_force_performance_level: auto
```

(`prepare_clocks_table` = auto→manual→`r`, `amd.rs:655-675`; commits collected in a
`VecDeque`, `c` written for each, `amd.rs:1377-1379`; performance level restored last,
`amd.rs:1381-1387`.)

### 1.3 Per-ASIC special-casing (what actually varies)

There are no per-gfxN quirk tables in the write path. The branches are:

- **GCN vs RDNA table format** — content sniffing of `pp_od_clk_voltage`
  (`overdrive/mod.rs:176-193`).
- **RDNA3 min>max write-order quirk** (`rdna.rs:50-78`).
- **RDNA4** — extra `OD_SCLK_OFFSET` and `OD_VDDGFX_OFFSET` sections parsed and written as
  `s {offset}` / `vo {offset}` (`rdna.rs:21-31, 257, 261, 80-85, 103-108`); RDNA4 cards do
  **not** expose `VDDC_CURVE` at all (see §3.2).
- **PMFW presence** — `family_id >= GC 11.0.0 (145)` → `HasPmfw` flag
  (`amd.rs:49, 769-775`); selects `gpu_od/fan_ctrl` fan path vs hwmon pwm path.
- **`normalize_vddc_curve`** for cards whose factory curve points are out of range
  (`rdna.rs:358-372`).
- **power level index off-by-one driver bug** (levels numbered from 1 instead of 0 after a
  sleep state): `normalize_power_level_indexes` (`amd.rs:469-494`, cites
  freedesktop work_items/5295); **duplicate `*` active markers** in `pp_dpm_*` handled in
  `parse_clock_levels` (`mod.rs:236-245`, cites work_items/5296).

## 2. READ/TELEMETRY surface

| Value | Node | Parse/units | Code |
|---|---|---|---|
| Socket power (avg / instant) | `H/power1_average`, `H/power1_input` | µW → W (`/1e6`) | `hw_mon.rs:86-102`; `amd.rs:1031-1032` |
| Power cap (current/max/min/default) | `H/power1_cap`, `power1_cap_max`, `power1_cap_min`, `power1_cap_default` | µW | `hw_mon.rs:105-128` |
| Edge/HotSpot/VRAM temps | `H/temp{i}_input` + `temp{i}_label`, plus `temp{i}_crit`, `temp{i}_crit_hyst` | millidegrees C /1000, keyed by label string (`"edge"`, `"hotspot"`, `"vram"`, …) | `hw_mon.rs:30-66`; `amd.rs:841-856` |
| Extra temps (soc, vrgfx, vrmem, vrsoc, skin) | `D/gpu_metrics` binary | per-struct fields; soc/skin ×100 fixed-point | `amd.rs:862-892` (via `libdrm_amdgpu_sys::GpuMetrics`) |
| Fan RPM (current/min/max/target) | `H/fan1_input`, `fan1_max`, `fan1_min`, `fan1_target`; fallback `gpu_metrics` `CurrentFanSpeed` | rpm | `hw_mon.rs:153-181`; `amd.rs:978-984` |
| Fan duty (current/min/max) | `H/pwm1`, `pwm1_min`, `pwm1_enable`; fallback `gpu_metrics` `FanPWM` + PMFW curve range | 0-255; PMFW range %→255 via `pct*2.55` | `hw_mon.rs:130-202`; `amd.rs:830-837, 985-991` |
| GFX clock (current) | `H/freq1_input` (Hz /1e6 → MHz); target from `gpu_metrics` `Gfxclk` | MHz | `hw_mon.rs:68-79`; `amd.rs:503-544` |
| UMC/mem clock | `H/freq2_input`; or `AMDGPU_SENSOR_INFO(GFX_MCLK)` ioctl | MHz | `hw_mon.rs:81-84`; `drm/amdgpu.rs:131-133` |
| GFX activity (%) | `D/gpu_busy_percent` (whole-GPU); per-process per-engine via `/proc/<pid>/fdinfo` (`drm-memory-vram`, gfx/dec/enc engine lines) | % | `mod.rs:173-177`; `amd.rs:1448-1456`, `common/fdinfo.rs` |
| UMC activity (%) | **not read** — no `mem_busy_percent` access anywhere in LACT (grep: only `gpu_busy_percent`) | — | — |
| VRAM used/total | `D/mem_info_vram_used`, `mem_info_vram_total` (bytes); GTT via ioctl `AMDGPU_INFO` | bytes | `mod.rs:158-171`; `drm/amdgpu.rs:123-129` |
| Throttle state | **only** `D/gpu_metrics` binary → `ThrottleStatus` bitmap, mapped to named bits (`ThrottlerBit`) | bits | `amd.rs:917-939` |
| Core/mem/pcie p-state levels + active | `D/pp_dpm_sclk`, `pp_dpm_mclk`, `pp_dpm_pcie` — lines `N: {MHz}Mhz`, active marked `*` | MHz | `mod.rs:203-278` (parser), `power_levels.rs:71-89` (filenames/suffixes) |
| OD table state (for readback/ranges) | `D/pp_od_clk_voltage` (whole text) | — | `mod.rs:303-307` |
| Power profile modes | `D/pp_power_profile_mode` (3 format variants: flat/basic/rotated/nested) | — | `power_profile_mode.rs:42-120+` |
| PMFW fan params readback | `D/gpu_od/fan_ctrl/*` (same 7 files as write surface, each file = `SECTION:\n<value>\nOD_RANGE:\nNAME: min max`) | rpm/°C/% | `mod.rs:406-482, 636-685`; `fan_control.rs:50-97` |
| Performance level | `D/power_dpm_force_performance_level` | string | `mod.rs:190-193` |
| VRAM voltage, SoC voltage | `H/in0_input`, `H/in1_input` (mV); `gpu_metrics` `VoltageMem/Soc` | mV | `hw_mon.rs:204-212`; `amd.rs:942-965` |
| VBIOS version | `D/vbios_version`; fuller via ioctl `AMDGPU_INFO` (ver+date) | string | `mod.rs:179-182`; `drm/amdgpu.rs:135-137`; `amd.rs:546-554` |
| PCIe link | parent-port `current/max_link_speed|width` (walks up 2 ports for the Navi10 PEX IDs `0x1478/0x1479`) or libdrm `PCI::BUS_INFO` | — | `mod.rs:109-156`; `amd.rs:596-634` |

`gpu_metrics` notes: it is a **versioned binary blob** (struct version embedded in header;
gfx11 → v2 layout). LACT does not parse it itself — the `libdrm_amdgpu_sys` crate
(`libdrm_amdgpu_sys` 0.8.16, `Cargo.toml:35-38`) does, reading `D/gpu_metrics`
(`GpuMetrics::get_from_sysfs_path`, `amd.rs:784, 821`). If a daemon needs throttle status
or per-domain power without this crate, it must parse the AMD GPU metrics struct
(`Documentation/gpu/amdgpu.gpu-metrics.rst`) itself — this is the one genuinely
undocumented-and-moving target.

## 3. Discovery of valid values (and: no VBIOS parsing)

### 3.1 How LACT learns max sclk/mclk/voltage, DPM levels, power-cap range

Everything from **driver-provided text sysfs**, read at runtime:

| Needed value | Source | Code |
|---|---|---|
| Allowed sclk range | `OD_RANGE:` → `SCLK: min max` in `pp_od_clk_voltage` (RDNA1-3) | `rdna.rs:279-284` |
| Allowed sclk **offset** range | `OD_RANGE:` → `SCLK_OFFSET: min max` (RDNA4) | `rdna.rs:283` |
| Allowed mclk range | `OD_RANGE:` → `MCLK: min max` | `rdna.rs:284` |
| Allowed VDDGFX offset range (undervolt window) | `OD_RANGE:` → `VDDGFX_OFFSET: min max` (RDNA3+) | `rdna.rs:285` |
| Per-point FV curve ranges | `OD_RANGE:` → `VDDC_CURVE_SCLK[i]: …`, `VDDC_CURVE_VOLT[i]: …` (also parsed from the `VDDC_CURVE:` section lines with `mv`) | `rdna.rs:264-274` |
| DPM clock levels + active | `pp_dpm_sclk` / `pp_dpm_mclk` text levels | `mod.rs:203-278` |
| Power cap range/default | `H/power1_cap_min` / `_max` / `_default` | `hw_mon.rs:116-128`; used in `amd.rs:1346-1375` |
| Fan curve ranges | `OD_RANGE:` inside `gpu_od/fan_ctrl/fan_curve` (`FAN_CURVE(hotspot temp): 25C 100C`, `FAN_CURVE(fan speed): 20% 100%`) | `mod.rs:661-679`; `fan_control.rs` tests show the format |
| Profile heuristic names/limits | `pp_power_profile_mode` table | `power_profile_mode.rs` |

`set_*` APIs validate against these ranges *before* writing
(`check_clockspeed_in_range`, `overdrive/mod.rs:142-156`; fan: `mod.rs:502-519`).

### 3.2 What this means for the R9700 (RDNA4, gfx11.0.3)

The only RDNA4 table LACT carries in tests is the RX 9070 XT (`tests/data/rx9070xt/pp_od_clk_voltage`):

```
OD_SCLK_OFFSET:
0Mhz
OD_MCLK:
0: 97Mhz
1: 1259MHz
OD_VDDGFX_OFFSET:
0mV
OD_RANGE:
SCLK_OFFSET:    -500Mhz       1000Mhz
MCLK:      97Mhz       1500Mhz
VDDGFX_OFFSET:    -200mv          0mv
```

Consequences for spec'ing your daemon (verify against your card — dump the file once on
target):

- **No `OD_SCLK` section, no `VDDC_CURVE`** → you cannot set an absolute max sclk or a
  custom FV curve on RDNA4 through this interface. SCLK is limited to
  `s {offset}` (−500…+1000 MHz); undervolt to `vo {offset}` (−200…0 mV, negatives only).
  LACT's `set_max_sclk` would fail with "GPU does not report allowed OD ranges"
  (`overdrive/mod.rs:152-154`) on such a table.
- MCLK min/max **is** settable (`m 0/1`, range 97…1500 MHz) even on RDNA4.
- If your R9700 VBIOS *does* expose `VDDC_CURVE` (driver is VBIOS-driven here), the same
  `vc i …` protocol applies.
- Fan: the 9070-class hwmon has **no `pwm1`/`fan1_*` at all** (see
  `tests/data/rx9070xt/hwmon/hwmon3/` — only temps, power, freq, in0). So on RDNA4-class
  cards the PMFW `gpu_od/fan_ctrl` path is the *only* fan write path; static % must be
  expressed as a flat 5-point curve (LACT does exactly that, `amd.rs:122-163`).
  `fan1_input`/`pwm1` readback comes from `gpu_metrics` instead.
- Family check for PMFW: `GC 11.0.0`+ (id 145) — `amd.rs:49`. R9700 qualifies.

### 3.3 What breaks if you get discovery wrong

- Writing outside `OD_RANGE` → driver rejects the line (`-EINVAL`/`-ERANGE` on the write or
  on `c`), LACT surfaces the error and the whole `apply_config` aborts (writes are staged;
  an uncommitted table is discarded, but a partially-accepted immediate-commit ASIC —
  e.g. sienna_cichlid — keeps what it took).
- Writing min > old max on RDNA3 → driver error (hence the reorder quirk).
- Ignoring the `manual` requirement → all `pp_od_*`/`pp_dpm_*`/`pp_power_profile_mode`
  writes fail.
- Assuming absolute sclk control on RDNA4 → silently no-op knob in LACT (config field
  exists but errors out / is dropped).

## 4. PPFEATUREMASK / OVERDRIVE

- Bit: `PP_OVERDRIVE_MASK = 0x4000`, `system.rs:32`. Read from
  `/sys/module/amdgpu/parameters/ppfeaturemask` (`system.rs:33, 180-188`).
- Check: `(mask & 0x4000) > 0` → `amdgpu_overdrive_enabled` in system info
  (`system.rs:56-60`). LACT **checks** the mask for reporting, but does **not** gate its
  per-write behavior on it — it just probes node availability (e.g.
  `get_clocks_table().is_err()` → skip clocks, `amd.rs:1149, 1419`;
  `get_fan_curve().is_ok()` → PMFW fan path, `amd.rs:123, 196, 791`).
- What the bit gates in-kernel: `pp_od_clk_voltage`, the whole `gpu_od/` directory
  (incl. `fan_ctrl`), legacy `pp_sclk_od`/`pp_mclk_od`. (`pp_dpm_*`, `pp_features`,
  `pp_power_profile_mode` and hwmon nodes are NOT gated by 0x4000.)
- LACT can flip the mask itself: writes
  `/etc/modprobe.d/99-amdgpu-overdrive.conf` with
  `options amdgpu ppfeaturemask=0x{new_mask:X}` (OR-ing in 0x4000), regenerates the
  initramfs (`update-initramfs -u` on Debian/Ubuntu, `system.rs:169-178`), requires reboot;
  rpm-ostree path via `kargs --append-if-missing` (`system.rs:107-116, 149-160`).
  Default Ubuntu kernel already ships 0x4000 enabled (default mask 0x8400), so in
  practice this is usually a no-op.
- Implication for your daemon: **just require/verify `0x4000` once at startup** (read the
  parameter file, tell the user to add the modprobe option + reboot if missing). No need
  to replicate LACT's toggling.

## 5. PERSISTENCE & RECOVERY

- **State stored**: single YAML, `/etc/lact/config.yaml` (root) or
  `~/.config/lact/config.yaml`, override `LACT_DAEMON_CONFIG_DIR`
  (`config.rs:463-477`). Contains per-GPU `GpuConfig` keyed by PCI-based id
  (`lact-schema/src/config.rs:37-57`: fan_control_enabled+settings, pmfw_options,
  power_cap, performance_level, clocks_configuration (min/max core/mem clock,
  voltage_offset, gpu_clock_offsets, vf curves), power_profile_mode_index,
  power_states, uma_carveout) plus named profiles + `current_profile`.
- **Boot**: systemd unit `res/lactd.service` (After=multi-user.target, Nice=-10,
  Restart=on-failure) → daemon loads config, enumerates GPUs (retries 5×3s if sysfs not
  ready yet, `handler.rs:139-157`), then **re-applies the whole config from scratch**
  (`handler.rs:221-223`). There is no "persist in driver" concept — the amdgpu driver
  resets all OD state on every GPU init, so the daemon is a *replier from a source of
  truth file*.
- **Suspend/resume**: logind `PrepareForSleep(start=false)` → `notify_reload_gpus(2s)`
  (`suspend.rs:9-36`) → `reload_gpus`: rebuild controllers, `cleanup()` +
  `reset_clocks()` old ones, re-apply config (`handler.rs:256-280`).
- **Driver reload / hotplug**: netlink uevent SUBSYSTEM=drm (500 ms debounce) → same
  `reload_gpus` path (`lib.rs:126-140`, `system.rs:273-323`).
- **Config file edited externally**: `notify` fs watcher, 50 ms debounce, reload +
  re-apply (`lib.rs:110-124`, `config.rs:376-440`).
- **Reset escape hatch**: boot arg `lact-reset` → backs up config to
  `config.reset-<ts>.yaml` and applies defaults (`handler.rs:95, 160-188`).
- Cleanup on shutdown: stop fan task, remove socket; nothing is written back to the GPU
  (it'll be reset by the driver anyway) (`lib.rs:94-108`).

For a two-identical-GPU setup: one config section per PCI slot id; apply the same values
to both; on any amdgpu reload both must be re-written (driver loses state on *both*).

## 6. POWER LIMIT DETAIL (gfx11)

LACT does **not** use `pp_dpm_ppt` at all (zero references in the tree). It uses the
**hwmon power cap**, which the amdgpu driver exposes in **microwatts**:

```rust
// amdgpu-sysfs/src/hw_mon.rs:109-113
pub fn set_power_cap(&self, cap: f64) -> Result<()> {
    let value = (cap * 1000000.0).round() as i64;
    self.write_file("power1_cap", value.to_string())
}
```

Called from `amd.rs:1343-1367`: reads `power1_cap_min`/`power1_cap_max` (µW→W),
`clamps` the requested watts, warns if clamped, writes
`D/hwmon/hwmonX/power1_cap` as integer µW (snapshot: 280 W → `280000000`). When no cap is
configured and current cap ≠ `power1_cap_default`, it re-asserts the default
(`amd.rs:1368-1375`). Readback: `power1_cap` (µW) → W.

`pp_dpm_ppt` (also µW in kernel, `D/pp_dpm_ppt`) is an equivalent, equally valid node, but
it is gated by DPM state and disappears when the GPU is runtime-suspended; hwmon
`power1_cap` is what LACT chose. Either works on gfx11; hwmon gives you min/max/default
readbacks for free.

## 7. EFFORT-SIZING INPUTS

### Line counts (wc -l)

| Module | Lines |
|---|---|
| amdgpu-sysfs total (incl. tests) | ~3,923 (`src` tree) |
| — `gpu_handle/mod.rs` (GpuHandle, fan_ctrl IO, perf levels, commit) | 1,090 |
| — `overdrive/rdna.rs` (RDNA table parse/write) | 799 |
| — `overdrive/gcn.rs` (legacy table) | 438 |
| — `overdrive/mod.rs` (trait + ranges + detection) | 420 |
| — `power_profile_mode.rs` (4 table formats) | 452 |
| — `hw_mon.rs` | 256 |
| — `fan_control.rs` (PMFW file parser) | 160 |
| — `power_levels.rs`, `sysfs.rs`, error | ~167 |
| LACT daemon AMD controller `amd.rs` | 1,830 |
| LACT `drm/amdgpu.rs` (ioctl provider) | 155 |
| LACT `common/fan_control.rs` (curve math/interp) | 240 |
| LACT `handler.rs` (shared, reload/apply logic) | 1,650 |
| LACT `config.rs` | 710 |
| LACT `system.rs` (ppfeaturemask, netlink) | 371 |
| LACT `suspend.rs` (logind) | 53 |
| **VBIOS parser modules** | **0 (do not exist in current LACT)** |
| **Per-ASIC quirk tables** | **0 dedicated tables**; just the branches listed in §1.3 |

### Write-surface node count

**18 distinct sysfs nodes** are written (table in §1); for an R9700-only daemon the
relevant set is 12: `pp_od_clk_voltage`(+`c`/`r`), `power_dpm_force_performance_level`,
`pp_dpm_sclk`, `pp_dpm_mclk`, `pp_power_profile_mode` (optional), hwmon `power1_cap`,
`pwm1`/`pwm1_enable` (likely absent — keep as fallback), and the 6 used
`gpu_od/fan_ctrl/*` files (+`c`/`r`).

### Risks / gotchas that make naive reimplementation unsafe

1. **`gpu_metrics` binary** — versioned per-ASIC struct; needed for throttle state, fan
   PWM/RPM fallback, per-domain power, target GFX clock. LACT outsources this to
   `libdrm_amdgpu_sys`. You can either reuse that crate or parse
   `Documentation/gpu/amdgpu.gpu-metrics.rst` yourself (struct v2 for gfx11). This is the
   most moving part of the whole surface.
2. **Stage-then-commit semantics** of `pp_od_clk_voltage` and `fan_curve` (write lines,
   then `c`); ASIC-dependent immediate commit. Always terminate with `c`; read back
   (`r` + re-read, or re-read table) to verify.
3. **RDNA3 min>max write-ordering quirk** and **out-of-range factory VDDC points**
   (must normalize before writing).
4. **Driver-side state loss on every GPU reset/suspend/driver reload** — your daemon
   needs LACT's whole reload+reapply machinery (netlink uevent + logind + config
   watcher) or the user silently loses settings after every hibernate.
5. **`pp_dpm_*` driver bugs**: 1-based level indexing after sleep state, duplicate `*`
   markers (LACT works around both, `amd.rs:469-494`, `mod.rs:236-245`).
6. **Fan task races**: LACT retries 10×, re-asserts `pwm1_enable=1` if someone flipped it
   to auto, delays fan spindown (`amd.rs:266-381`). If you do a userspace curve loop on
   pre-RDNA3 cards you inherit this; on R9700 you mostly don't (PMFW does the loop).
7. **Sysfs hygiene**: null bytes in some file contents (`trim_sysfs_line`,
   `mod.rs:832-835`), `power1_*` in µW (easy ×1000 mistake), temps in millidegrees,
   freqs in Hz.
8. **Permissions**: all writes need root (or amdgpu group + capable user). LACT layers
   polkit/socket-permissions on top for a setuid-free client design; a root daemon skips
   all of that.
9. **Rejection semantics**: an out-of-range line returns an error from `write()` or from
   the `c` commit; LACT treats any error as fatal for that apply and rolls the config
   state back to the previous profile (`handler.rs:340-422`). Verify-by-readback after
   `c` (re-read `pp_od_clk_voltage`) is cheap insurance LACT itself doesn't do.

## 8. REIMPLEMENTATION SIZE ASSESSMENT (gfx11, two identical R9700s)

### (a) Core sysfs control — ~1-2 days

For R9700 specifically the surface collapses: no GCN format, no VDDC curve (verify!), no
hwmon pwm (verify!), no UMA. What you actually need:

- Parse `pp_od_clk_voltage` RDNA format (sections + OD_RANGE) — port of
  `overdrive/rdna.rs:228-343` (~120 lines minus tests) or write from the kernel doc.
- Write `s {offset}` / `m 0|1 {hz}` / `vo {mv}` + `r`/`c` protocol — ~50 lines.
- `power_dpm_force_performance_level` auto→manual→…→restore dance — ~30 lines.
- hwmon `power1_cap` µW set + min/max clamp — ~20 lines.
- `gpu_od/fan_ctrl`: parse `SECTION:/value/OD_RANGE:` files, write value + `c`, curve
  `i temp speed` — ~100 lines.
- Static fan % → flat 5-point curve conversion — ~20 lines.
- Telemetry: hwmon temps/power/fan/pwm + `gpu_busy_percent` + `mem_info_vram_*` +
  `pp_dpm_*` active levels — ~150 lines.
- Throttle state + extras: `gpu_metrics` parse — **either link
  `libdrm_amdgpu_sys`/libdrm (0 effort, what LACT does) or hand-parse the v2 struct
  (~200 lines, risky)**.

Total: **~500-700 lines** of Rust (or equivalent), plus a UDS/CLI front. This is a
weekend job, not a month.

### (b) VBIOS parsing — 0 effort (for your feature set)

Current LACT does **not** parse the VBIOS at all; every range you need arrives pre-parsed
in `OD_RANGE`, `pp_dpm_*`, and `power1_cap_*`. The only VBIOS touch is dumping
`/sys/kernel/debug/dri/<N>/amdgpu_vbios` for display. If you later want absolute max-sclk
or FV-curve control on RDNA4 (which the driver doesn't expose), *that* would require
VBIOS work — but LACT can't do it either, so you'd be building new capability, not
porting. **Skip VBIOS entirely.**

### (c) Robustness / persistence — ~3-5 days (the real bulk)

- Config file (YAML/JSON) with per-slot-id sections + hot reload (file watcher).
- Startup re-apply with early-boot retry.
- Reload triggers: netlink `KOBJECT_UEVENT` (drm) with debounce; logind
  `PrepareForSleep`; amdgpu module reload; manual command. This is LACT's
  `handler.rs` reload path (~200 lines) + `system.rs` netloop (~50) + `suspend.rs` (~50).
- Apply-atomicity: reset (`r`) → write → commit (`c`) → restore perf level; error →
  abort + report; verify-by-readback.
- Stock/default readback on demand (`r` + read table, `power1_cap_default`,
  `fan_curve` read).
- Startup self-checks: ppfeaturemask 0x4000 present, `gpu_od/fan_ctrl` present, hwmon
  present, family ≥ GC11 — with actionable error messages.

### Bottom line

For two identical gfx11 R9700s with your feature list (power cap, static % + curve fan,
sclk/mclk limits, voltage offset, telemetry), the honest size is **~1,000-1,500 lines of
daemon code; 1-2 weeks part-time** end-to-end: (a) a few days, (b) nothing, (c) a few
days. The **single biggest risk** is the `gpu_metrics` binary for throttle status/fan
readback (undocumented-ish, versioned per ASIC) — mitigate by reusing the
`libdrm_amdgpu_sys` crate exactly like LACT, or by treating throttle info as
best-effort. Second-biggest risk: assuming the R9700's `pp_od_clk_voltage` matches the
9070 test data (offset-only, no FV curve) — dump the file from your card on day 1 and
spec against it.
