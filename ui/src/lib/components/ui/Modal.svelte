<script lang="ts">
  import { closeModal, isOpen } from "../../store/modal.svelte";
  import type { Snippet } from "svelte";

  let {
    id,
    flavor = "card",
    title,
    children,
  }: {
    id: string;
    flavor?: "card" | "sheet";
    title?: string;
    children: Snippet;
  } = $props();

  const open = $derived(isOpen(id));
  // svelte-ignore non_reactive_update
  let panel: HTMLDivElement | null = null;
  let lastFocused: Element | null = null;

  const FOCUSABLE =
    'a[href],button:not([disabled]),textarea,input,select,[tabindex]:not([tabindex="-1"])';

  function trapTab(e: KeyboardEvent) {
    if (!panel) return;
    const f = panel.querySelectorAll<HTMLElement>(FOCUSABLE);
    if (f.length === 0) return;
    const first = f[0];
    const last = f[f.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  $effect(() => {
    if (!open) return;
    lastFocused = document.activeElement;
    const t = requestAnimationFrame(() =>
      panel?.querySelector<HTMLElement>(FOCUSABLE)?.focus(),
    );
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") closeModal();
      if (e.key === "Tab") trapTab(e);
    }
    document.addEventListener("keydown", onKey);
    return () => {
      cancelAnimationFrame(t);
      document.removeEventListener("keydown", onKey);
      (lastFocused as HTMLElement | null)?.focus?.();
    };
  });
</script>

{#if open}
  <div
    class="fixed inset-0 z-50 flex {flavor === 'sheet' ? 'items-end' : 'items-center justify-center'} p-4"
  >
    <button
      type="button"
      tabindex="-1"
      aria-label="close dialog"
      class="absolute inset-0 cursor-default border-0 bg-scrim"
      onclick={closeModal}
    ></button>
    <div
      bind:this={panel}
      role="dialog"
      aria-modal="true"
      aria-label={title}
      class="relative z-10 w-full {flavor === 'sheet' ? 'max-w-2xl' : 'max-w-md'} rounded-panel border border-hairline bg-surface p-4 text-sm text-text"
    >
      {#if title}
        <div class="mb-3 flex items-center justify-between">
          <h2 class="text-text">{title}</h2>
          <button
            aria-label="close"
            title="close"
            class="text-faint hover:text-text"
            onclick={closeModal}
          >
            ✕
          </button>
        </div>
      {/if}
      {@render children()}
    </div>
  </div>
{/if}
