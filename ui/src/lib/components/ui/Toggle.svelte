<script lang="ts">
  import { getContext } from "svelte";
  import type { HTMLInputAttributes } from "svelte/elements";

  let {
    checked = $bindable(false),
    id,
    readonly = false,
    class: className = "",
    ...rest
  }: {
    checked?: boolean;
    id?: string;
    readonly?: boolean;
    class?: string;
  } & HTMLInputAttributes = $props();

  const fieldId = getContext<string | undefined>("amd-gpu-tuned:fieldId");
  const inputId = $derived(id ?? fieldId);
</script>

<label class="relative inline-flex h-5 w-8 cursor-pointer items-center">
  <input
    type="checkbox"
    role="switch"
    bind:checked
    id={inputId}
    class="peer sr-only"
    disabled={readonly}
    {...rest}
  />
  <span
    class="h-5 w-8 rounded-pill bg-surface-2 transition-colors duration-[var(--duration-fast)] ease-[var(--ease-out)] peer-checked:bg-accent"
  ></span>
  <span
    class="absolute left-1 h-4 w-4 rounded-pill bg-text transition-transform duration-[var(--duration-fast)] ease-[var(--ease-out)] peer-checked:translate-x-3"
  ></span>
  <span class="sr-only {className}">toggle</span>
</label>
