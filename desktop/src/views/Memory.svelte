<script>
  import { onMount } from 'svelte';
  import { runCommand } from '../lib/api.js';

  let query = $state('');
  let output = $state('');
  let loading = $state(false);
  let mode = $state('semantic');

  async function search(args) {
    loading = true;
    try {
      output = await runCommand(['memory', ...args]);
    } catch (e) {
      output = `Error: ${e}`;
    } finally {
      loading = false;
    }
  }

  function doSearch() {
    const q = query.trim();
    const args = q ? [q] : [];
    if (mode === 'semantic') args.push('--semantic');
    if (mode === 'recent' || !q) args.push('--recent');
    search(args.length ? args : ['--recent']);
  }

  onMount(() => search(['--recent']));
</script>

<div class="view">
  <header>
    <h1 class="glow-text">⬡ Memory</h1>
    <p class="sub">Persistent, queryable, versioned. Nothing dies.</p>
  </header>

  <div class="controls">
    <input bind:value={query} placeholder="Search memory…" onkeydown={(e) => e.key === 'Enter' && doSearch()} />
    <select bind:value={mode}>
      <option value="semantic">Semantic</option>
      <option value="recent">Recent</option>
      <option value="keyword">Keyword</option>
    </select>
    <button class="primary" onclick={doSearch} disabled={loading}>
      {loading ? '…' : 'Search'}
    </button>
  </div>

  <pre class="output card">{loading ? 'Querying the vault…' : output || 'No results.'}</pre>
</div>

<style>
  .view { display: flex; flex-direction: column; height: 100%; max-width: 1000px; }
  header h1 { color: var(--neon-cyan); font-size: 24px; }
  .sub { color: var(--text-dim); font-size: 12px; margin-top: 4px; }
  .controls { display: flex; gap: 10px; margin: 16px 0; }
  .controls input { flex: 1; }
  .output { flex: 1; overflow-y: auto; font-size: 12px; line-height: 1.5; min-height: 0; }
</style>
