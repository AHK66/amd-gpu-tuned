<script lang="ts">
  import { toasts, dismiss } from "../../store/toast.svelte";

  const TONE: Record<string, string> = {
    success: "border-success/40 text-success",
    error: "border-danger/40 text-danger",
    info: "border-info/40 text-info",
  };
</script>

<div class="pointer-events-none fixed bottom-4 right-4 z-50 flex flex-col gap-2">
  {#each toasts as t (t.id)}
    <button
      class="pointer-events-auto rounded-control border bg-surface px-3 py-2 text-sm {TONE[t.tone]}"
      onclick={() => dismiss(t.id)}
    >
      {t.msg}
    </button>
  {/each}
</div>
