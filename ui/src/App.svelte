<script lang="ts">
  import { onSseError, onSseMessage, rings, samples } from "./lib/store/telemetry.svelte";
  import { route, setNavGuard } from "./lib/router.svelte";
  import { hasPending } from "./lib/store/pending.svelte";
  import { openModal } from "./lib/store/modal.svelte";
  import Sidebar from "./lib/components/ui/Sidebar.svelte";
  import Toast from "./lib/components/ui/Toast.svelte";
  import UnsavedChangesDialog from "./lib/components/ui/UnsavedChangesDialog.svelte";
  import UnlockDialog from "./lib/components/UnlockDialog.svelte";
  import Dashboard from "./routes/Dashboard.svelte";
  import Presets from "./routes/Presets.svelte";
  import Drift from "./routes/Drift.svelte";
  import Setup from "./routes/Setup.svelte";

  $effect(() => {
    const es = new EventSource("/api/telemetry/stream");
    es.onmessage = onSseMessage;
    es.onerror = onSseError;
    return () => es.close();
  });

  // Block leaving the Dashboard while a tile has unsaved preset changes; the
  // dialog lets the user save, discard, or stay.
  $effect(() => {
    setNavGuard(
      (target) => route() === "" && target !== "" && hasPending(),
      () => openModal("unsaved-changes"),
    );
    return () => setNavGuard(null);
  });
</script>

<div class="flex h-screen bg-bg text-text">
  <Sidebar />
  <main class="flex-1 overflow-auto">
    {#if route() === ""}
      <Dashboard {samples} {rings} />
    {:else if route() === "presets"}
      <Presets />
    {:else if route() === "drift"}
      <Drift />
    {:else}
      <Setup />
    {/if}
  </main>
  <Toast />
  <UnsavedChangesDialog />
  <UnlockDialog />
</div>
