// WebSocket helpers for the openshark serve API.
//
// Protocol (from openshark src/api/ws.rs):
//   client → { type: 'chat', message, model? }        on /ws/v1/chat
//   client → { type: 'agent', task, yolo, max_turns } on /ws/v1/agent
//   client → { type: 'ping' }
//   server → { type: 'pong' }
//          | { type: 'thinking', content }
//          | { type: 'token', content }
//          | { type: 'tool_call', name, args, turn }
//          | { type: 'tool_result', name, output, success, turn }
//          | { type: 'error', message }
//          | { type: 'complete', summary, total_turns, duration_secs }

/**
 * Open a WS to the openshark server.
 * handlers: { onOpen(ws), onMessage(msg, ws), onClose(), onError(err) }
 * Returns the WebSocket.
 */
export function sharkWs(port, path, handlers = {}) {
  const ws = new WebSocket(`ws://127.0.0.1:${port}${path}`);
  ws.onopen = () => handlers.onOpen?.(ws);
  ws.onmessage = (e) => {
    try {
      handlers.onMessage?.(JSON.parse(e.data), ws);
    } catch {
      // non-JSON frame — ignore
    }
  };
  ws.onclose = () => handlers.onClose?.();
  ws.onerror = (err) => handlers.onError?.(err);
  return ws;
}

/**
 * Stateful filter that strips <think>…</think> blocks from a token stream,
 * handling tags split across tokens. Feed tokens in, get displayable text out.
 */
export function makeThinkFilter() {
  let buf = '';
  let inThink = false;
  return function feed(token) {
    buf += token;
    let out = '';
    while (buf.length > 0) {
      if (inThink) {
        const end = buf.indexOf('</think>');
        if (end === -1) {
          // hold only a tail that could complete the closing tag
          buf = buf.slice(-8);
          break;
        }
        buf = buf.slice(end + 8);
        inThink = false;
      } else {
        const start = buf.indexOf('<think>');
        if (start === -1) {
          // hold a tail that could grow into '<think>'
          const partial = buf.match(/<(?:t(?:h(?:i(?:n(?:k)?)?)?)?)?$/);
          const cut = partial ? buf.length - partial[0].length : buf.length;
          out += buf.slice(0, cut);
          buf = buf.slice(cut);
          break;
        }
        out += buf.slice(0, start);
        buf = buf.slice(start + 7);
        inThink = true;
      }
    }
    return out;
  };
}
