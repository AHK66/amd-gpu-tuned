<script lang="ts">
  import Card from "../../../src/lib/components/ui/Card.svelte";
</script>

<Card>
  {#snippet title()}
    <h3>Header</h3>
  {/snippet}
  body content
</Card>
