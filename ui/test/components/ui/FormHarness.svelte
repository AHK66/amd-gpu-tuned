<script lang="ts">
  import Field from "../../../src/lib/components/ui/Field.svelte";
  import TextInput from "../../../src/lib/components/ui/TextInput.svelte";
  import Select from "../../../src/lib/components/ui/Select.svelte";
  import Toggle from "../../../src/lib/components/ui/Toggle.svelte";

  let name = $state("");
  let mode = $state("static");
  let on = $state(true);
</script>

<Field label="name" hint="the preset name">
  <TextInput bind:value={name} data-testid="name-input" />
</Field>

<Select bind:value={mode} data-testid="mode-select">
  <option value="static">static</option>
  <option value="curve">curve</option>
</Select>

<Toggle bind:checked={on} data-testid="toggle" aria-label="enable" />
