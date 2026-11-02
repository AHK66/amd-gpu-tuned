<script lang="ts">
  import Badge from "../../../src/lib/components/ui/Badge.svelte";
  let { tone = "neutral" }: { tone?: "neutral" | "info" | "success" | "warn" | "danger" } = $props();
</script>

<Badge {tone}>label</Badge>
