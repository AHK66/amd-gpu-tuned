<script lang="ts">
  import LineChart from "../../../src/lib/components/charts/LineChart.svelte";
  import type {
    LineSeries,
    ChartReference,
    SecondaryAxis,
  } from "../../../src/lib/components/charts/LineChart.svelte";

  let {
    series = [],
    unit = "",
    title = "Test Chart",
    yDomain,
    xDomain,
    height = 160,
    references = [],
    secondary,
  }: {
    series?: LineSeries[];
    unit?: string;
    title?: string;
    yDomain?: [number, number];
    xDomain?: [number, number];
    height?: number;
    references?: ChartReference[];
    secondary?: SecondaryAxis;
  } = $props();
</script>

<div style="width: 600px">
  <LineChart {series} {unit} {title} {yDomain} {xDomain} {height} {references} {secondary} />
</div>
