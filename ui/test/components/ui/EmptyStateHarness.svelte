<script lang="ts">
  import EmptyState from "../../../src/lib/components/ui/EmptyState.svelte";
  import { Cpu } from "@lucide/svelte";
</script>

<EmptyState icon={Cpu} heading="Waiting" subtext="no data yet" />
