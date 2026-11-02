<script lang="ts">
  import Table from "../../../src/lib/components/ui/Table.svelte";
  import Th from "../../../src/lib/components/ui/Th.svelte";
</script>

<Table>
  {#snippet header()}
    <Th>name</Th>
    <Th>updated</Th>
  {/snippet}
  <tr>
    <td>e2e</td>
    <td>just now</td>
  </tr>
</Table>
