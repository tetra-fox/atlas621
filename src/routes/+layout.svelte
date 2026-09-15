<script lang="ts">
  import "../app.css";

  let { children } = $props();
</script>

<main class="relative min-h-svh">
  {@render children()}
</main>
