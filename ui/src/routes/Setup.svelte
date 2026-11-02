<script lang="ts">
  import { getDriftSetup, getRingMinutes, getSetup, setDriftSetup, setRingMinutes } from "../lib/api";
  import Badge from "../lib/components/ui/Badge.svelte";
  import Card from "../lib/components/ui/Card.svelte";
  import Toggle from "../lib/components/ui/Toggle.svelte";
  import Tooltip from "../lib/components/ui/Tooltip.svelte";
  import { toast } from "../lib/store/toast.svelte";
  import { authState } from "../lib/store/auth.svelte";
  import { enabledFeatureCount } from "../lib/logic/ppmask";
  import type { GpuSetup, SetupCheck } from "../lib/types";

  let chk = $state<SetupCheck | null>(null);
  let ringMin = $state(10);

  const ringTip =
    "How long per-GPU realtime samples are kept in the daemon's in-memory ring buffer. Charts show at most this much history. Applied immediately; survives restarts.";

  $effect(() => {
    getRingMinutes()
      .then((r) => (ringMin = r.minutes))
      .catch((e) => toast.error(String(e)));
  });

  let ringCommitting = false;
  let ringTimer: ReturnType<typeof setTimeout> | undefined;

  // Input commits after 1s of quiet; every change restarts the timer.
  function scheduleCommit() {
    clearTimeout(ringTimer);
    ringTimer = setTimeout(() => commitNow(), 1000);
  }

  $effect(() => () => clearTimeout(ringTimer));

  // Commits the current value when it is a valid integer 1–60; silently
  // skips invalid values (mid-typing states) — Enter shows the toast.
  async function commitNow() {
    if (ringCommitting) return;
    const n = Number(ringMin);
    if (!Number.isInteger(n) || n < 1 || n > 60) return;
    ringCommitting = true;
    try {
      const r = await setRingMinutes(n);
      ringMin = r.minutes;
      toast.success(`History window set to ${r.minutes} minutes`);
    } catch (err) {
      toast.error(String(err));
      ringMin = (await getRingMinutes().catch(() => null))?.minutes ?? ringMin;
    } finally {
      ringCommitting = false;
    }
  }

  async function commitRing(e: KeyboardEvent) {
    if (e.key !== "Enter") return;
    clearTimeout(ringTimer);
    const n = Number(ringMin);
    if (!Number.isInteger(n) || n < 1 || n > 60) {
      toast.error("enter a whole number between 1 and 60");
      return;
    }
    await commitNow();
  }

  let driftOn = $state(true);
  let driftDelay = $state(30);

  const driftTip =
    "When a card's settings are changed outside amd-gpu-tuned (e.g. it reboots itself in stock state), restore the last applied settings after a delay. Off = adopt the external values as today.";

  $effect(() => {
    getDriftSetup()
      .then((d) => {
        driftOn = d.reapply;
        driftDelay = d.delay_s;
      })
      .catch((e) => toast.error(String(e)));
  });

  let driftCommitting = false;
  let driftTimer: ReturnType<typeof setTimeout> | undefined;

  function scheduleDriftCommit() {
    clearTimeout(driftTimer);
    driftTimer = setTimeout(() => commitDriftNow(), 1000);
  }

  $effect(() => () => clearTimeout(driftTimer));

  function validDelay(): number | null {
    const n = Number(driftDelay);
    return Number.isInteger(n) && n >= 5 && n <= 600 ? n : null;
  }

  async function commitDriftNow() {
    if (driftCommitting) return;
    const n = validDelay();
    if (n == null) return;
    driftCommitting = true;
    try {
      const r = await setDriftSetup({ reapply: driftOn, delay_s: n });
      driftOn = r.reapply;
      driftDelay = r.delay_s;
      toast.success(`Restore delay set to ${r.delay_s} s`);
    } catch (err) {
      toast.error(String(err));
      const d = await getDriftSetup().catch(() => null);
      if (d) {
        driftOn = d.reapply;
        driftDelay = d.delay_s;
      }
    } finally {
      driftCommitting = false;
    }
  }

  async function commitDriftToggle() {
    if (driftCommitting) return;
    driftCommitting = true;
    try {
      const r = await setDriftSetup({ reapply: driftOn, delay_s: validDelay() ?? driftDelay });
      driftOn = r.reapply;
      driftDelay = r.delay_s;
    } catch (err) {
      toast.error(String(err));
      const d = await getDriftSetup().catch(() => null);
      if (d) {
        driftOn = d.reapply;
        driftDelay = d.delay_s;
      }
    } finally {
      driftCommitting = false;
    }
  }

  function commitDriftDelay(e: KeyboardEvent) {
    if (e.key !== "Enter") return;
    clearTimeout(driftTimer);
    const n = validDelay();
    if (n == null) {
      toast.error("enter a whole number between 5 and 600");
      return;
    }
    void commitDriftNow();
  }

  $effect(() => {
    getSetup().then((c) => (chk = c)).catch((e) => toast.error(String(e)));
  });

  const gates: { label: string; tip: string; fn: (g: GpuSetup) => boolean }[] = [
    {
      label: "overdrive table",
      tip: "Overdrive clock/voltage offset support exposed in sysfs. Required to tune GPU core & memory clocks.",
      fn: (g) => g.overdrive_table,
    },
    {
      label: "pmfw fan",
      tip: "The pmfw (power-management firmware) fan interface is available for reading/setting fan speed.",
      fn: (g) => g.pmfw_fan,
    },
    {
      label: "hwmon",
      tip: "A sysfs hwmon node is present for temperature & power sensor readings.",
      fn: (g) => g.hwmon,
    },
    {
      label: "dpm levels",
      tip: "DPM (Dynamic Power Management) clock levels are readable, so available clock steps can be listed.",
      fn: (g) => g.dpm_levels,
    },
  ];

  function mark(ok: boolean): string {
    return ok ? "yes" : "NO";
  }

  const ppTip = $derived(
    chk?.live_mask != null
      ? `Live PPFeaturesMask: 0x${chk.live_mask.toString(16)}. A bitmask of AMD power-play features; the overdrive bit must be set for clock tuning.`
      : "The PPFeaturesMask could not be read from the card."
  );
</script>

<div class="p-4 text-sm text-text">
  {#if !authState.enabled}
    <p class="mb-3 text-xs text-faint" data-testid="no-password-note">
      no write password set —
      <code class="font-mono">amd-gpu-tuned set-password</code> to require one for
      changes.
    </p>
  {/if}
  {#if chk}
    <div class="mb-3 flex items-center gap-3">
      <Badge tone={chk.ready ? "success" : "danger"} data-testid="setup-ready">
        {chk.ready ? "READY" : "NOT READY"}
      </Badge>
      <Tooltip text={ppTip}>
        <span class="text-xs text-faint">
          PP features: {chk.live_mask != null ? enabledFeatureCount(chk.live_mask) + " enabled" : "unreadable"}
        </span>
      </Tooltip>
    </div>

    {#each chk.gpus as g (g.slot)}
      <Card class="mb-3">
        <p class="mb-2 text-xs text-faint">PCI address: <span class="font-mono">{g.slot}</span></p>
        <div class="grid grid-cols-2 gap-2 text-xs md:grid-cols-4">
          {#each gates as gate (gate.label)}
            <span>
              <Tooltip text={gate.tip}><span>{gate.label}</span></Tooltip>:
              <span class={gate.fn(g) ? "text-faint" : "text-danger"}>{mark(gate.fn(g))}</span>
            </span>
          {/each}
        </div>
      </Card>
    {/each}
  {:else}
    <p class="text-faint">loading setup check…</p>
  {/if}

  <Card class="mt-3">
    <div class="mb-2 flex items-center gap-2">
      <span class="text-xs text-faint">Telemetry history</span>
      <Tooltip text={ringTip}>
        <span class="text-xs text-faint">info</span>
      </Tooltip>
    </div>
    <div class="flex items-center gap-2 text-xs">
      <label for="ring-minutes">ring window (minutes)</label>
      <input
        id="ring-minutes"
        type="text"
        inputmode="numeric"
        bind:value={ringMin}
        oninput={() => scheduleCommit()}
        onkeydown={commitRing}
        data-testid="ring-minutes"
        class="w-20 rounded-md border border-hairline bg-surface-2 px-2 py-1 text-sm text-text"
      />
    </div>
  </Card>

  <Card class="mt-3">
    <div class="mb-2 flex items-center gap-2">
      <span class="text-xs text-faint">Drift recovery</span>
      <Tooltip text={driftTip}>
        <span class="text-xs text-faint">info</span>
      </Tooltip>
    </div>
    <div class="flex items-center gap-2 text-xs">
      <Toggle
        id="drift-reapply"
        bind:checked={driftOn}
        onchange={commitDriftToggle}
        data-testid="drift-reapply-toggle"
      />
      <label for="drift-reapply">Auto-restore after third-party change</label>
    </div>
    <div class="mt-2 flex items-center gap-2 text-xs" class:opacity-40={!driftOn} class:pointer-events-none={!driftOn}>
      <label for="drift-delay">restore delay (seconds)</label>
      <input
        id="drift-delay"
        type="text"
        inputmode="numeric"
        bind:value={driftDelay}
        disabled={!driftOn}
        oninput={scheduleDriftCommit}
        onkeydown={commitDriftDelay}
        data-testid="drift-delay"
        class="w-20 rounded-md border border-hairline bg-surface-2 px-2 py-1 text-sm text-text"
      />
    </div>
  </Card>
</div>
