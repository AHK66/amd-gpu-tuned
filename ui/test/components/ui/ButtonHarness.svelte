<script lang="ts">
  import Button from "../../../src/lib/components/ui/Button.svelte";
  import { Cpu } from "@lucide/svelte";

  let { variant = "secondary", pill = false }: {
    variant?: "primary" | "secondary" | "ghost";
    pill?: boolean;
  } = $props();
</script>

<Button {variant} {pill} icon={Cpu} data-testid="btn">save</Button>
