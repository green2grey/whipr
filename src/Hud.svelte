<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { theme } from './lib/theme';
  import RecordingHud from './lib/components/RecordingHud.svelte';

  // Ensure theme side effects run (theme store applies immediately on import).
  theme.subscribe(() => {});

  onMount(() => {
    document.body.classList.add('hud');
    document.documentElement.classList.add('hud');
  });

  onDestroy(() => {
    document.body.classList.remove('hud');
    document.documentElement.classList.remove('hud');
  });
</script>

<RecordingHud />
