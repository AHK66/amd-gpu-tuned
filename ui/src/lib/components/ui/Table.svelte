<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLTableAttributes } from "svelte/elements";

  let {
    header,
    children,
    class: className = "",
    ...rest
  }: {
    header: Snippet;
    children: Snippet;
    class?: string;
  } & HTMLTableAttributes = $props();
</script>

<table class="w-full text-sm text-text {className}" {...rest}>
  <thead>
    <tr>
      {@render header()}
    </tr>
  </thead>
  <tbody>
    {@render children()}
  </tbody>
</table>
