<script>
  import { onMount } from 'svelte';
  import { getBinaryInfo, serverStart, serverStatus } from './lib/api.js';
  import { THEMES, applyTheme, currentThemeId } from './lib/themes.js';
  import Dashboard from './views/Dashboard.svelte';
  import Chat from './views/Chat.svelte';
  import Agent from './views/Agent.svelte';
  import Memory from './views/Memory.svelte';
  import Models from './views/Models.svelte';
  import Tools from './views/Tools.svelte';
  import Doctor from './views/Doctor.svelte';
  import Config from './views/Config.svelte';

  const tabs = [
    { id: 'dashboard', label: 'Dashboard', icon: '◈' },
    { id: 'chat', label: 'Chat', icon: '▸' },
    { id: 'agent', label: 'Agent', icon: '⚡' },
    { id: 'memory', label: 'Memory', icon: '⬡' },
    { id: 'models', label: 'Models', icon: '◆' },
    { id: 'tools', label: 'Tools', icon: '⚙' },
    { id: 'doctor', label: 'Doctor', icon: '✚' },
    { id: 'config', label: 'Config', icon: '≡' },
  ];

  let active = $state('dashboard');
  let binary = $state(null);
  let binaryError = $state(null);
  let themeId = $state(currentThemeId());
  let showThemes = $state(false);
  let server = $state(null);

  function pickTheme(id) {
    applyTheme(id);
    themeId = id;
    showThemes = false;
  }

  onMount(async () => {
    try {
      binary = await getBinaryInfo();
    } catch (e) {
      binaryError = String(e);
    }
    // Boot the API server in the background — views fall back to CLI without it.
    try {
      server = await serverStart();
    } catch {
      try {
        server = await serverStatus();
      } catch {
        server = null;
      }
    }
  });
</script>

<div class="shell">
  <aside class="sidebar">
    <div class="logo">
      <div class="wordmark">
        <span class="word-open">OPEN</span>
        <span class="word-shark">SHARK</span>
      </div>
      <span class="tagline">Fast. Precise. Hungry.</span>
    </div>

    <nav>
      {#each tabs as tab}
        <button
          class:active={active === tab.id}
          onclick={() => (active = tab.id)}
        >
          <span class="icon">{tab.icon}</span>
          {tab.label}
        </button>
      {/each}
    </nav>

    <div class="status">
      {#if binary?.found}
        <span class="badge ok">v{binary.version ?? '?'}</span>
        <span class="path" title={binary.path}>{binary.path}</span>
      {:else if binary || binaryError}
        <span class="badge err">binary missing</span>
        <span class="path">install openshark</span>
      {:else}
        <span class="badge info">detecting…</span>
      {/if}

      <button class="theme-toggle" onclick={() => (showThemes = !showThemes)}>
        🎨 {THEMES.find((t) => t.id === themeId)?.name ?? 'Themes'}
      </button>

      {#if server?.running}
        <span class="badge ok server-badge" title="openshark serve — WebSocket streaming active">
          ⚡ api :{server.port}
        </span>
      {/if}

      {#if showThemes}
        <div class="theme-picker">
          {#each THEMES as theme}
            <button
              class="theme-option"
              class:active={theme.id === themeId}
              onclick={() => pickTheme(theme.id)}
              title={theme.desc}
            >
              <span class="theme-icon">{theme.icon}</span>
              <span class="theme-name">{theme.name}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </aside>

  <main>
    {#if active === 'dashboard'}
      <Dashboard {binary} />
    {:else if active === 'chat'}
      <Chat />
    {:else if active === 'agent'}
      <Agent />
    {:else if active === 'memory'}
      <Memory />
    {:else if active === 'models'}
      <Models />
    {:else if active === 'tools'}
      <Tools />
    {:else if active === 'doctor'}
      <Doctor />
    {:else if active === 'config'}
      <Config />
    {/if}
  </main>
</div>

<style>
  .shell {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    width: 220px;
    flex-shrink: 0;
    background: var(--bg-panel);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    padding: 16px 10px;
  }

  .logo {
    padding: 0 6px 16px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 12px;
  }

  .wordmark {
    display: flex;
    flex-direction: column;
    align-items: center;
    font-size: 24px;
    line-height: 1.1;
    letter-spacing: 2px;
    margin-bottom: 4px;
  }

  .word-open {
    color: var(--neon-cyan);
    text-shadow: 0 0 8px rgba(0, 229, 255, 0.5);
  }

  .word-shark {
    color: var(--neon-pink);
    text-shadow: 0 0 8px rgba(255, 45, 120, 0.5);
  }

  .tagline {
    display: block;
    text-align: center;
    font-size: 10px;
    color: var(--neon-pink);
    font-style: italic;
    opacity: 0.85;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
  }

  nav button {
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    padding: 10px 12px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-dim);
  }

  nav button:hover {
    color: var(--text);
    background: var(--bg-elevated);
    box-shadow: none;
    border-color: transparent;
  }

  nav button.active {
    background: var(--bg-elevated);
    border-color: var(--neon-pink);
    color: var(--neon-pink);
    box-shadow: 0 0 12px rgba(255, 45, 120, 0.2);
  }

  .icon {
    width: 16px;
    text-align: center;
  }

  .status {
    padding-top: 12px;
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: flex-start;
  }

  .path {
    font-size: 10px;
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
    white-space: nowrap;
  }

  .theme-toggle {
    width: 100%;
    text-align: left;
    padding: 8px 10px;
    font-size: 12px;
    color: var(--text-dim);
  }

  .theme-toggle:hover {
    color: var(--neon-cyan);
  }

  .theme-picker {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 280px;
    overflow-y: auto;
    width: 100%;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 4px;
  }

  .theme-option {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    padding: 6px 8px;
    font-size: 12px;
    background: transparent;
    border-color: transparent;
    color: var(--text-dim);
  }

  .theme-option:hover {
    background: var(--bg-panel);
    color: var(--text);
  }

  .theme-option.active {
    border-color: var(--neon-pink);
    color: var(--neon-pink);
  }

  .theme-icon {
    width: 18px;
    text-align: center;
  }

  .server-badge {
    font-size: 10px;
  }

  main {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    background:
      radial-gradient(ellipse at top, rgba(185, 103, 255, 0.05), transparent 60%),
      var(--bg);
  }
</style>
