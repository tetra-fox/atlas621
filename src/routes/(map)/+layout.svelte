<script lang="ts">
  import MapApp from "$lib/map/MapApp.svelte";
  import { MapState, setMapState } from "$lib/map/state.svelte";

  let { children } = $props();
  setMapState(new MapState());
</script>

<MapApp />
{@render children()}
