<script lang="ts">
  import PresetEditor from "../../src/lib/components/PresetEditor.svelte";
  import type { SlotForm } from "../../src/lib/logic/form";
  import { STATIC } from "../logic/thermals.test";

  let {
    name = $bindable(""),
    notes = $bindable(""),
    sourceForm,
    readonly = false,
  }: {
    name?: string;
    notes?: string;
    sourceForm: SlotForm;
    readonly?: boolean;
  } = $props();

  // Flat, scalar-only shape: a shallow copy is a fully independent reactive
  // copy of the caller's (non-reactive) test object. (The indirection mirrors
  // the pre-existing harness pattern and keeps the prop read out of init.)
  function seedForm(): SlotForm {
    return { ...sourceForm };
  }
  let form = $state<SlotForm>(seedForm());

  $effect(() => {
    Object.assign(sourceForm, form);
  });
</script>

<PresetEditor bind:name bind:notes {form} static={STATIC} {readonly} />
