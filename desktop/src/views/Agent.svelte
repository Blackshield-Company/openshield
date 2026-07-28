<script>
  import { onMount } from 'svelte';
  import { runStream } from '../lib/api.js';
  import { sharkWs } from '../lib/sharkws.js';
  import { resolveConn } from '../lib/conn.js';

  let task = $state('');
  let yolo = $state(false);
  let running = $state(false);
  let outputEl;
  let conn = $state(null);

  // Structured event log (server mode) or raw text (cli mode)
  let events = $state([]);
  let cliOutput = $state('');

  onMount(async () => {
    conn = await resolveConn();
    if (!conn.running) conn = null;
  });

  function push(ev) {
    events = [...events, ev];
    requestAnimationFrame(() => {
      if (outputEl) outputEl.scrollTop = outputEl.scrollHeight;
    });
  }

  async function run() {
    const t = task.trim();
    if (!t || running) return;
    running = true;
    events = [];
    cliOutput = '';

    // Re-check in case the connection came up after this view mounted
    if (!conn) {
      const c = await resolveConn();
      if (c.running) conn = c;
    }

    if (conn) {
      await runViaServer(t);
    } else {
      await runViaCli(t);
    }
    running = false;
  }

  /** Structured execution via /ws/v1/agent — thinking, tool calls, results. */
  function runViaServer(t) {
    return new Promise((resolve) => {
      let settled = false;
      const done = () => {
        if (!settled) {
          settled = true;
          resolve();
        }
      };

      push({ kind: 'meta', text: `$ openshark agent "${t}" (${conn.mode} ${conn.host}:${conn.port})` });

      sharkWs(conn.host, conn.port, '/ws/v1/agent', {
        onOpen: (ws) => {
          ws.send(JSON.stringify({ type: 'agent', task: t, yolo, max_turns: 50 }));
        },
        onMessage: (m, ws) => {
          switch (m.type) {
            case 'thinking':
              push({ kind: 'thinking', text: m.content });
              break;
            case 'tool_call':
              push({ kind: 'tool_call', name: m.name, args: m.args, turn: m.turn });
              break;
            case 'tool_result':
              push({ kind: 'tool_result', name: m.name, output: m.output, success: m.success, turn: m.turn });
              break;
            case 'token':
              push({ kind: 'thinking', text: m.content });
              break;
            case 'error':
              push({ kind: 'error', text: m.message });
              ws.close();
              done();
              break;
            case 'complete':
              push({ kind: 'complete', text: m.summary, turns: m.total_turns, secs: m.duration_secs });
              ws.close();
              done();
              break;
          }
        },
        onClose: done,
        onError: () => {
          push({ kind: 'error', text: 'connection error — is openshark serve running?' });
          done();
        },
      });
    });
  }

  /** Fallback: raw CLI stream. */
  async function runViaCli(t) {
    cliOutput = `$ openshark agent "${t}"\n\n`;
    try {
      await runStream(['agent', t], (ev) => {
        if (ev.event === 'stdout' || ev.event === 'stderr') {
          cliOutput += ev.data;
        } else if (ev.event === 'done') {
          cliOutput += `\n\n[process exited with code ${ev.data}]`;
        }
        requestAnimationFrame(() => {
          if (outputEl) outputEl.scrollTop = outputEl.scrollHeight;
        });
      });
    } catch (e) {
      cliOutput += `\nError: ${e}`;
    }
  }
</script>

<div class="view">
  <header>
    <h1 class="glow-text">⚡ Agent</h1>
    <p class="sub">Autonomous task execution — plan, tool, ship.</p>
  </header>

  <div class="controls">
    <input bind:value={task} placeholder="Describe the task…" onkeydown={(e) => e.key === 'Enter' && run()} />
    <label class="yolo" title="Skip confirmations — full send">
      <input type="checkbox" bind:checked={yolo} /> yolo
    </label>
    <button class="primary" onclick={run} disabled={running || !task.trim()}>
      {running ? 'Running…' : 'Run Agent'}
    </button>
  </div>

  <div class="output card" bind:this={outputEl}>
    {#if conn}
      {#if events.length === 0}
        <p class="placeholder">Events will stream here.</p>
      {/if}
      {#each events as ev}
        {#if ev.kind === 'meta'}
          <p class="ev meta">{ev.text}</p>
        {:else if ev.kind === 'thinking'}
          <p class="ev thinking">💭 {ev.text}</p>
        {:else if ev.kind === 'tool_call'}
          <p class="ev tool-call">🔧 <strong>{ev.name}</strong> <span class="turn">turn {ev.turn}</span></p>
          <pre class="ev args">{ev.args}</pre>
        {:else if ev.kind === 'tool_result'}
          <p class="ev tool-result" class:fail={!ev.success}>
            {ev.success ? '✅' : '❌'} <strong>{ev.name}</strong> <span class="turn">turn {ev.turn}</span>
          </p>
          <pre class="ev result">{ev.output}</pre>
        {:else if ev.kind === 'complete'}
          <p class="ev complete">🏁 Done — {ev.turns} turns in {ev.secs}s</p>
          <pre class="ev summary">{ev.text}</pre>
        {:else if ev.kind === 'error'}
          <p class="ev error">⚠ {ev.text}</p>
        {/if}
      {/each}
      {#if running}<span class="cursor">▊</span>{/if}
    {:else}
      <pre class="cli">{cliOutput || 'Output will stream here. (Start the server for structured events.)'}</pre>
    {/if}
  </div>
</div>

<style>
  .view { display: flex; flex-direction: column; height: 100%; max-width: 1000px; }
  header h1 { color: var(--neon-cyan); font-size: 24px; }
  .sub { color: var(--text-dim); font-size: 12px; margin-top: 4px; }
  .controls { display: flex; gap: 10px; margin: 16px 0; align-items: center; }
  .controls input:not([type='checkbox']) { flex: 1; }
  .yolo { color: var(--neon-yellow); font-size: 12px; display: flex; align-items: center; gap: 4px; }

  .output { flex: 1; overflow-y: auto; font-size: 12px; line-height: 1.5; min-height: 0; color: var(--text); }
  .placeholder { color: var(--text-dim); }
  .cli { color: var(--text); }

  .ev { margin: 4px 0; }
  .ev.meta { color: var(--text-dim); }
  .ev.thinking { color: var(--neon-purple); font-style: italic; }
  .ev.tool-call { color: var(--neon-cyan); }
  .ev.tool-result { color: var(--success); }
  .ev.tool-result.fail { color: var(--error); }
  .ev.complete { color: var(--success); font-weight: 600; }
  .ev.error { color: var(--error); }
  .ev.args, .ev.result, .ev.summary {
    background: var(--bg-elevated);
    border-radius: 6px;
    padding: 8px 10px;
    margin: 2px 0 8px;
    font-size: 11px;
    max-height: 200px;
    overflow-y: auto;
  }
  .turn { color: var(--text-dim); font-size: 10px; margin-left: 6px; }

  .cursor { color: var(--neon-cyan); animation: blink 1s step-end infinite; }
  @keyframes blink { 50% { opacity: 0; } }
</style>
