<script lang="ts">
  import type { Preset } from "../types";
  import Badge from "./ui/Badge.svelte";
  import Table from "./ui/Table.svelte";
  import Th from "./ui/Th.svelte";
  import { Copy, Trash2 } from "@lucide/svelte";

  let {
    presets,
    onOpen,
    onDelete,
    onDuplicate,
  }: {
    presets: Preset[];
    onOpen: (p: Preset) => void;
    onDelete: (p: Preset) => void;
    onDuplicate: (p: Preset) => void;
  } = $props();
</script>

<Table>
  {#snippet header()}
    <Th>actions</Th>
    <Th>name</Th>
    <Th>updated</Th>
  {/snippet}
  {#each presets as p (p.id ?? p.name)}
    <tr class="border-t border-hairline hover:bg-surface-2">
      <td>
        <span class="flex gap-2">
          <button class="text-muted hover:text-text" title="duplicate" aria-label="duplicate" onclick={() => onDuplicate(p)}><Copy size={16} /></button>
          <button class="text-muted hover:text-danger" title="delete" aria-label="delete" onclick={() => onDelete(p)}><Trash2 size={16} /></button>
        </span>
      </td>
      <td>
        <button data-testid="preset-row-{p.id}" class="text-text hover:text-accent" onclick={() => onOpen(p)}>
          {p.name}
        </button>
        {#if p.is_builtin}
          <Badge tone="info" data-testid="preset-badge-builtin">built-in</Badge>
        {/if}
      </td>
      <td class="text-xs text-faint">{p.updated_at ? new Date(p.updated_at).toLocaleString() : "—"}</td>
    </tr>
  {/each}
</Table>
