<script lang="ts">
  import Modal from "./Modal.svelte";
  import Button from "./Button.svelte";
  import { closeModal } from "../../store/modal.svelte";
  import { Trash2 } from "@lucide/svelte";

  let {
    id,
    title,
    body,
    confirmLabel = "Confirm",
    variant = "primary",
    onConfirm,
  }: {
    id: string;
    title: string;
    body: string;
    confirmLabel?: string;
    variant?: "primary" | "secondary" | "ghost" | "danger";
    onConfirm: () => void;
  } = $props();
</script>

<Modal {id} {title}>
  <p class="text-muted">{body}</p>
  <div class="mt-4 flex justify-end gap-2">
    <Button variant="ghost" onclick={closeModal}>cancel</Button>
    <Button
      variant={variant}
      icon={Trash2}
      onclick={() => {
        onConfirm();
        closeModal();
      }}
    >
      {confirmLabel}
    </Button>
  </div>
</Modal>
