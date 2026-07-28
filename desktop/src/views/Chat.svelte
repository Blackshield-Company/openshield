<script>
  import { onMount } from 'svelte';
  import { runStream } from '../lib/api.js';
  import { sharkWs, makeThinkFilter } from '../lib/sharkws.js';
  import { resolveConn } from '../lib/conn.js';

  let input = $state('');
  let messages = $state([]);
  let running = $state(false);
  let outputEl;
  let conn = $state(null);
  let model = $state('');
  let activeWs = null;

  onMount(async () => {
    conn = await resolveConn();
    if (!conn.running) conn = null;
  });

  function appendToLast(text) {
    messages[messages.length - 1].text += text;
    scrollDown();
  }

  async function send() {
    const msg = input.trim();
    if (!msg || running) return;
    input = '';
    running = true;

    messages = [...messages, { role: 'user', text: msg }];
    messages = [...messages, { role: 'assistant', text: '' }];
    scrollDown();

    // Re-check in case the connection came up after this view mounted
    if (!conn) {
      const c = await resolveConn();
      if (c.running) conn = c;
    }

    if (conn) {
      await sendViaServer(msg);
    } else {
      await sendViaCli(msg);
    }
    running = false;
  }

  /** Streaming via openshark serve WebSocket — real token deltas. */
  function sendViaServer(msg) {
    return new Promise((resolve) => {
      const filter = makeThinkFilter();
      let settled = false;
      const done = () => {
        if (!settled) {
          settled = true;
          resolve();
        }
      };

      activeWs = sharkWs(conn.host, conn.port, '/ws/v1/chat', {
        onOpen: (ws) => {
          const payload = { type: 'chat', message: msg };
          if (model.trim()) payload.model = model.trim();
          ws.send(JSON.stringify(payload));
        },
        onMessage: (m, ws) => {
          if (m.type === 'token') {
            appendToLast(filter(m.content));
          } else if (m.type === 'complete') {
            ws.close();
            done();
          } else if (m.type === 'error') {
            appendToLast(`\n[error] ${m.message}`);
            ws.close();
            done();
          }
          // thinking/pong: no-op
        },
        onClose: done,
        onError: () => {
          appendToLast('\n[connection error — is openshark serve running?]');
          done();
        },
      });
    });
  }

  /** Fallback: spawn the CLI and stream stdout lines. */
  async function sendViaCli(msg) {
    const args = ['chat'];
    if (model.trim()) args.push('-m', model.trim());
    args.push(msg);
    try {
      await runStream(args, (ev) => {
        if (ev.event === 'stdout') {
          appendToLast(ev.data);
        } else if (ev.event === 'done' && ev.data !== '0') {
          appendToLast(`\n[exited with code ${ev.data}]`);
        }
      });
    } catch (e) {
      appendToLast(`Error: ${e}`);
    }
  }

  function scrollDown() {
    requestAnimationFrame(() => {
      if (outputEl) outputEl.scrollTop = outputEl.scrollHeight;
    });
  }

  function onKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }
</script>

<div class="view">
  <header>
    <h1 class="glow-text">▸ Chat</h1>
    <div class="chat-bar">
      <span class="badge {conn ? 'ok' : 'info'}">
        {conn ? (conn.mode === 'remote' ? `🌐 ${conn.host}:${conn.port}` : `server :${conn.port}`) : 'cli mode'}
      </span>
      <input
        class="model-input"
        bind:value={model}
        placeholder="model (blank = server default)"
        title="Model override, e.g. synthclaw-fast"
      />
    </div>
  </header>

  <div class="transcript" bind:this={outputEl}>
    {#if messages.length === 0}
      <p class="empty">Ask the shark anything. It bites fast.</p>
    {/if}
    {#each messages as m}
      <div class="msg {m.role}">
        <span class="who">{m.role === 'user' ? 'you' : '🦈 openshark'}</span>
        <pre>{m.text}{#if running && m === messages[messages.length - 1]}<span class="cursor">▊</span>{/if}</pre>
      </div>
    {/each}
  </div>

  <div class="composer">
    <textarea
      bind:value={input}
      onkeydown={onKeydown}
      placeholder="Message openshark… (Enter to send)"
      rows="2"
    ></textarea>
    <button class="primary" onclick={send} disabled={running || !input.trim()}>
      {running ? '…' : 'Send'}
    </button>
  </div>
</div>

<style>
  .view { display: flex; flex-direction: column; height: 100%; max-width: 900px; }
  header h1 { color: var(--neon-cyan); font-size: 24px; margin-bottom: 8px; }

  .chat-bar { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
  .model-input { width: 260px; font-size: 11px; padding: 6px 10px; }

  .transcript {
    flex: 1;
    overflow-y: auto;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-height: 0;
  }

  .empty { color: var(--text-dim); font-size: 13px; }

  .who { font-size: 11px; text-transform: uppercase; letter-spacing: 1px; display: block; margin-bottom: 4px; }
  .msg.user .who { color: var(--neon-yellow); }
  .msg.assistant .who { color: var(--neon-pink); }
  .msg pre { font-size: 13px; line-height: 1.5; }

  .cursor { color: var(--neon-cyan); animation: blink 1s step-end infinite; }
  @keyframes blink { 50% { opacity: 0; } }

  .composer { display: flex; gap: 10px; margin-top: 12px; }
  textarea { flex: 1; resize: none; }
</style>
