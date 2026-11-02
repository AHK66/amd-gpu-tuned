#!/usr/bin/env python3
"""Prometheus exporter for AMD GPUs based on the `amd-smi` CLI (ROCm 7).

Exposes temperatures, fan speed, power, clocks, utilization, ECC counters
and throttle state on GET /metrics. Stdlib only.
"""

import argparse
import csv
import glob
import io
import os
import re
import subprocess
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = 9457
GPU_TUNED_URL = "http://127.0.0.1:8787"
AMDSMI = ["amd-smi"]
METRIC_CMD = ["metric", "-t", "-f", "-p", "-u", "-e", "-c", "--csv"]


def run(cmd, timeout=30):
    proc = subprocess.run(
        AMDSMI + cmd, capture_output=True, text=True, timeout=timeout
    )
    if proc.returncode != 0:
        raise RuntimeError(f"amd-smi {' '.join(cmd)} failed: {proc.stderr.strip()}")
    return proc.stdout


def load_gpu_identity():
    identity = {}
    bdfs = {}
    gpu = None
    for line in run(["list"]).splitlines():
        line = line.strip()
        m = re.match(r"GPU:\s*(\d+)", line)
        if m:
            gpu = m.group(1)
            identity[gpu] = {"bdf": "unknown", "asic_serial": "unknown"}
            continue
        m = re.match(r"BDF:\s*(\S+)", line)
        if m and gpu is not None:
            bdfs[gpu] = m.group(1)
    serials = []
    for line in run(["static"]).splitlines():
        line = line.strip()
        m = re.match(r"ASIC_SERIAL:\s*(0x[0-9a-fA-F]+)", line)
        if m:
            serials.append(m.group(1).lower().removeprefix("0x"))
    for gpu, info in identity.items():
        info["bdf"] = bdfs.get(gpu, "unknown")
        info["asic_serial"] = serials[int(gpu)] if int(gpu) < len(serials) else "unknown"
    return identity


IDENTITY = load_gpu_identity()


def load_vram_paths():
    """Map gpu index -> (used_bytes_path, total_bytes_path).

    amdgpu exposes per-card VRAM accounting under
    /sys/class/drm/cardN/device/mem_info_vram_{used,total}. Cards are
    matched to GPUs by PCI BDF (the device symlink's basename).
    """
    bdf_to_gpu = {
        info["bdf"]: gpu for gpu, info in IDENTITY.items() if info["bdf"] != "unknown"
    }
    vram = {}
    for card in glob.glob("/sys/class/drm/card*/device/mem_info_vram_used"):
        device = os.path.realpath(os.path.dirname(card))
        gpu = bdf_to_gpu.get(os.path.basename(device))
        if gpu is None:
            continue
        vram[gpu] = (card, os.path.join(device, "mem_info_vram_total"))
    return vram


VRAM = load_vram_paths()


def read_int(path):
    try:
        with open(path) as fh:
            return int(fh.read().strip())
    except (OSError, ValueError):
        return None


def fetch_gpu_tuned(url):
    """Fetch gpu_tune_* lines from the daemon; [] on any failure."""
    if not url:
        return []
    try:
        with urllib.request.urlopen(url.rstrip("/") + "/api/exporter/metrics", timeout=5) as r:
            text = r.read().decode()
    except Exception:
        return []
    lines = []
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        try:
            _head, value = line.rsplit(" ", 1)
            float(value)
        except ValueError:
            continue
        lines.append(line)
    return lines


def f(row, col):
    try:
        return float(row[col])
    except (ValueError, KeyError):
        return None


def first_numeric(row, *cols):
    for col in cols:
        val = f(row, col)
        if val is not None:
            return val
    return None


def scrape_metrics():
    started = time.time()
    out = run(METRIC_CMD)
    lines = []
    rows = list(csv.DictReader(io.StringIO(out)))
    for row in rows:
        gpu = row["gpu"].strip()
        ident = IDENTITY.get(gpu, {"bdf": "unknown", "asic_serial": "unknown"})
        labels = f'gpu="{gpu}",bdf="{ident["bdf"]}",asic_serial="{ident["asic_serial"]}"'

        def emit(name, value, extra=""):
            if value is None:
                return
            l = f"{labels},{extra}" if extra else labels
            lines.append(f"{name}{{{l}}} {value}")

        emit("amd_gpu_info", 1)
        for col, mtype in (("edge", "edge"), ("hotspot", "hotspot"), ("mem", "mem")):
            emit("amd_gpu_temp_celsius", f(row, col), f'type="{mtype}"')
        emit("amd_gpu_fan_rpm", f(row, "rpm"))
        emit("amd_gpu_fan_percent", f(row, "usage"))
        emit("amd_gpu_socket_power_watts", f(row, "socket_power"))
        emit("amd_gpu_gfx_clock_mhz", first_numeric(row, "gfx_0_clk", "gfx_1_clk"))
        emit("amd_gpu_utilization_percent", f(row, "gfx_activity"), 'engine="gfx"')
        emit("amd_gpu_utilization_percent", f(row, "umc_activity"), 'engine="umc"')
        emit("amd_gpu_ecc_correctable_total", f(row, "total_correctable_count"))
        emit("amd_gpu_ecc_uncorrectable_total", f(row, "total_uncorrectable_count"))
        emit("amd_gpu_ecc_deferred_total", f(row, "total_deferred_count"))
        emit(
            "amd_gpu_throttle_latched",
            1 if row.get("throttle_status", "").strip() == "THROTTLED" else 0,
        )
        paths = VRAM.get(gpu)
        if paths:
            emit("amd_gpu_mem_used_bytes", read_int(paths[0]))
            emit("amd_gpu_mem_total_bytes", read_int(paths[1]))
        emit("amd_gpu_up", 1)
    lines.append(f"amd_gpu_exporter_scrape_seconds {time.time() - started:.3f}")
    lines.append("")
    tuned = fetch_gpu_tuned(GPU_TUNED_URL)
    if tuned:
        lines.extend(tuned)
    else:
        lines.append("gpu_tune_up 0")
    return "\n".join(lines)


HELP = {
    "amd_gpu_info": "GPU identity (1 = present).",
    "amd_gpu_temp_celsius": "GPU temperature in Celsius (type=edge|hotspot|mem).",
    "amd_gpu_fan_rpm": "Fan speed in RPM.",
    "amd_gpu_fan_percent": "Fan speed as percentage.",
    "amd_gpu_socket_power_watts": "Socket power in watts.",
    "amd_gpu_utilization_percent": "Engine utilization percent (engine=gfx|umc).",
    "amd_gpu_ecc_correctable_total": "Total correctable ECC errors (counter).",
    "amd_gpu_ecc_uncorrectable_total": "Total uncorrectable ECC errors (counter).",
    "amd_gpu_ecc_deferred_total": "Total deferred ECC errors (counter).",
    "amd_gpu_throttle_latched": "Driver throttle flag; latched, stays set after any throttle event.",
    "amd_gpu_mem_used_bytes": "VRAM used in bytes (amdgpu sysfs mem_info_vram_used).",
    "amd_gpu_mem_total_bytes": "Total VRAM in bytes (amdgpu sysfs mem_info_vram_total).",
    "amd_gpu_up": "1 if the GPU reported data in the last scrape.",
}


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/metrics":
            try:
                body = scrape_metrics()
                code, ctype = 200, "text/plain; version=0.0.4"
            except Exception as exc:
                body = f"# exporter error: {exc}\n"
                code, ctype = 500, "text/plain"
            data = body.encode()
            self.send_response(code)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        elif self.path == "/health":
            data = b"ok\n"
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, fmt, *args):
        pass


def main():
    global GPU_TUNED_URL
    ap = argparse.ArgumentParser()
    ap.add_argument("--amd-gpu-tuned-url", default="http://127.0.0.1:8787",
                    help="amd-gpu-tuned daemon base URL (empty to disable)")
    args = ap.parse_args()
    GPU_TUNED_URL = args.amd_gpu_tuned_url
    server = ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
    print(f"amd-smi exporter listening on :{PORT}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
