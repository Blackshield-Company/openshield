<script>
  import { onMount } from 'svelte';
  import { runCommand } from '../lib/api.js';

  let output = $state('');
  let loading = $state(true);

  onMount(async () => {
    try {
      output = await runCommand(['tools', 'list']);
    } catch (e) {
      output = `Error: ${e}`;
    } finally {
      loading = false;
    }
  });
</script>

<div class="view">
  <header>
    <h1 class="glow-text">⚙ Tools</h1>
    <p class="sub">Every weapon in the arsenal.</p>
  </header>
  <pre class="output card">{loading ? 'Loading tools…' : output || 'No tools found.'}</pre>
</div>

<style>
  .view { display: flex; flex-direction: column; height: 100%; max-width: 1000px; }
  header h1 { color: var(--neon-cyan); font-size: 24px; }
  .sub { color: var(--text-dim); font-size: 12px; margin-top: 4px; }
  .output { margin-top: 16px; overflow-y: auto; font-size: 12px; line-height: 1.5; }
</style>
