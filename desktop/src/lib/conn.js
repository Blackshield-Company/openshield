// Connection resolution: local server (managed by the Rust backend) or a
// remote openshark serve over the network (phone → desktop over LAN).
//
// localStorage 'openshark-remote' = ""            → local mode
//                                   = "host:port" → remote mode

import { serverStatus } from './api.js';

const KEY = 'openshark-remote';

export function getRemote() {
  return localStorage.getItem(KEY) ?? '';
}

export function setRemote(value) {
  const v = (value ?? '').trim();
  if (v) localStorage.setItem(KEY, v);
  else localStorage.removeItem(KEY);
}

/** Parse "host:port" → { host, port } (port defaults to 1984). */
export function parseRemote(value) {
  const [host, portStr] = value.split(':');
  return { host: host.trim(), port: parseInt(portStr, 10) || 1984 };
}

/** Health-check a remote server via HTTP. */
export async function remoteHealth(host, port) {
  try {
    const res = await fetch(`http://${host}:${port}/api/v1/health`, {
      signal: AbortSignal.timeout(4000),
    });
    if (!res.ok) return null;
    const json = await res.json();
    return json.version ?? 'unknown';
  } catch {
    return null;
  }
}

/**
 * Resolve the active connection.
 * → { mode: 'local'|'remote', host, port, version, running }
 * Never throws; running=false when nothing is reachable.
 */
export async function resolveConn() {
  const remote = getRemote();
  if (remote) {
    const { host, port } = parseRemote(remote);
    const version = await remoteHealth(host, port);
    return { mode: 'remote', host, port, version, running: version !== null };
  }
  try {
    const s = await serverStatus();
    if (s.running) {
      return { mode: 'local', host: '127.0.0.1', port: s.port, version: s.version, running: true };
    }
  } catch {
    // backend unavailable (e.g. mobile build without local server support)
  }
  return { mode: 'local', host: '127.0.0.1', port: 1984, version: null, running: false };
}
