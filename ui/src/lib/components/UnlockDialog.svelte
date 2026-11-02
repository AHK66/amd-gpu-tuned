<script lang="ts">
  import Modal from "./ui/Modal.svelte";
  import Button from "./ui/Button.svelte";
  import { ApiError, unlock } from "../api";
  import { closeModal } from "../store/modal.svelte";
  import { toast } from "../store/toast.svelte";

  let password = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function submit() {
    if (busy || password.length === 0) return;
    busy = true;
    error = null;
    try {
      await unlock(password);
      password = "";
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        error = "wrong password";
      } else {
        toast.error(String(e));
      }
    } finally {
      busy = false;
    }
  }
</script>

<Modal id="unlock" title="Unlock writes">
  <form
    onsubmit={(e) => {
      e.preventDefault();
      submit();
    }}
  >
    <p class="mb-2 text-xs text-faint">
      Write access is locked. Enter the write password to unlock.
    </p>
    <input
      type="password"
      data-testid="unlock-password"
      bind:value={password}
      placeholder="write password"
      class="w-full rounded-md border border-hairline bg-surface-2 px-2 py-1 text-sm text-text"
    />
    {#if error}
      <p class="mt-2 text-xs text-danger" data-testid="unlock-error">
        {error}
      </p>
    {/if}
    <div class="mt-4 flex justify-end gap-2">
      <Button variant="ghost" onclick={closeModal}>cancel</Button>
      <Button variant="primary" type="submit" disabled={busy || password.length === 0}>
        unlock
      </Button>
    </div>
  </form>
</Modal>
