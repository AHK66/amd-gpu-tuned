<script lang="ts">
  import SegmentedTabs from "../../../src/lib/components/ui/SegmentedTabs.svelte";

  let value = $state("static");
  const options = [
    { value: "static", label: "static" },
    { value: "curve", label: "curve" },
  ];
</script>

<SegmentedTabs bind:value {options} />
