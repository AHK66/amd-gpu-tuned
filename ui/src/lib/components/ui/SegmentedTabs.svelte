<script lang="ts" generics="V extends string">
  let {
    value = $bindable("") as V,
    options,
    class: className = "",
  }: {
    value?: V;
    options: { value: V; label: string }[];
    class?: string;
  } = $props();
</script>

<div role="tablist" class="inline-flex rounded-control border border-hairline bg-bg p-0.5 {className}">
  {#each options as o (o.value)}
    <button
      role="tab"
      aria-selected={value === o.value}
      class="rounded-control px-3 py-1 text-xs {value === o.value ? "bg-surface-2 text-text" : "text-muted hover:text-text"}"
      onclick={() => (value = o.value)}
    >
      {o.label}
    </button>
  {/each}
</div>
