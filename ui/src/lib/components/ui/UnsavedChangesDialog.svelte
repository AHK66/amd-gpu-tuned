<script lang="ts">
  import Modal from "./Modal.svelte";
  import Button from "./Button.svelte";
  import { closeModal } from "../../store/modal.svelte";
  import { discardAll, pendingLabels, saveAll } from "../../store/pending.svelte";
  import { blockedRoute, proceed } from "../../router.svelte";

  let busy = $state(false);

  const labels = $derived(pendingLabels());

  async function act(kind: "save" | "discard") {
    if (busy) return;
    busy = true;
    try {
      const ok = kind === "save" ? await saveAll() : await discardAll();
      if (!ok) return; // an apply failed — stay put; the tile is still registered
      closeModal();
      const target = blockedRoute();
      if (target != null) proceed(target);
    } finally {
      busy = false;
    }
  }
</script>

<Modal id="unsaved-changes" title="Unsaved preset changes">
  <p class="text-muted">
    {#if labels.length > 0}
      You have unsaved changes to {labels.length} preset{labels.length === 1 ? "" : "s"}:
      <span class="text-text">{labels.join(", ")}</span>.
    {:else}
      You have unsaved preset changes.
    {/if}
    If you leave without saving, they'll be lost.
  </p>
  <div class="mt-4 flex justify-end gap-2">
    <Button variant="ghost" data-testid="unsaved-stay" disabled={busy} onclick={closeModal}>Stay</Button>
    <Button variant="danger" data-testid="unsaved-discard" disabled={busy} onclick={() => act("discard")}>Discard</Button>
    <Button variant="primary" data-testid="unsaved-save" disabled={busy} onclick={() => act("save")}>Save</Button>
  </div>
</Modal>
