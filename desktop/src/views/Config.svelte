<script>
  import { onMount } from 'svelte';
  import { readConfig, writeConfig } from '../lib/api.js';

  let content = $state('');
  let original = $state('');
  let loading = $state(true);
  let saving = $state(false);
  let message = $state('');
  let dirty = $derived(content !== original);

  let cfgPath = $state('');

  onMount(async () => {
    try {
      const cfg = await readConfig();
      content = cfg.content;
      original = cfg.content;
      cfgPath = cfg.path;
    } catch (e) {
      message = `Error: ${e}`;
    } finally {
      loading = false;
    }
  });

  async function save() {
    saving = true;
    message = '';
    try {
      await writeConfig(content);
      original = content;
      message = 'Saved. The shark remembers.';
    } catch (e) {
      message = `Error: ${e}`;
    } finally {
      saving = false;
    }
  }
</script>

<div class="view">
  <header>
    <h1 class="glow-text">≡ Config</h1>
    <p class="sub">{cfgPath || '~/.config/openshark/config.toml'}</p>
  </header>

  {#if loading}
    <p class="hint">Loading config…</p>
  {:else}
    <textarea class="editor" bind:value={content} spellcheck="false"></textarea>
    <div class="bar">
      <button class="primary" onclick={save} disabled={saving || !dirty}>
        {saving ? 'Saving…' : dirty ? 'Save Changes' : 'Saved'}
      </button>
      {#if message}<span class="msg">{message}</span>{/if}
    </div>
  {/if}
</div>

<style>
  .view { display: flex; flex-direction: column; height: 100%; max-width: 1000px; }
  header h1 { color: var(--neon-cyan); font-size: 24px; }
  .sub { color: var(--text-dim); font-size: 12px; margin-top: 4px; }
  .hint { color: var(--text-dim); margin-top: 16px; }
  .editor {
    flex: 1;
    margin-top: 16px;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 16px;
    font-size: 12px;
    line-height: 1.6;
    resize: none;
    min-height: 0;
  }
  .bar { display: flex; align-items: center; gap: 12px; margin-top: 12px; }
  .msg { font-size: 12px; color: var(--success); }
</style>
