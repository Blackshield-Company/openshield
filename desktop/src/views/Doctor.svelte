<script>
  import { onMount } from 'svelte';
  import { runCommand } from '../lib/api.js';

  let output = $state('');
  let loading = $state(true);
  let fixing = $state(false);

  async function diagnose() {
    loading = true;
    try {
      output = await runCommand(['doctor']);
    } catch (e) {
      output = String(e);
    } finally {
      loading = false;
    }
  }

  async function fix() {
    fixing = true;
    try {
      output = await runCommand(['doctor', '--fix']);
    } catch (e) {
      output = String(e);
    } finally {
      fixing = false;
    }
  }

  onMount(diagnose);
</script>

<div class="view">
  <header>
    <h1 class="glow-text">✚ Doctor</h1>
    <p class="sub">Diagnostics. If it hurts, the shark finds it.</p>
  </header>

  <div class="controls">
    <button onclick={diagnose} disabled={loading || fixing}>Re-run</button>
    <button class="primary" onclick={fix} disabled={loading || fixing}>
      {fixing ? 'Fixing…' : 'Fix All'}
    </button>
  </div>

  <pre class="output card">{loading ? 'Running diagnostics…' : output || 'All clear.'}</pre>
</div>

<style>
  .view { display: flex; flex-direction: column; height: 100%; max-width: 1000px; }
  header h1 { color: var(--neon-cyan); font-size: 24px; }
  .sub { color: var(--text-dim); font-size: 12px; margin-top: 4px; }
  .controls { display: flex; gap: 10px; margin: 16px 0; }
  .output { flex: 1; overflow-y: auto; font-size: 12px; line-height: 1.5; min-height: 0; }
</style>
