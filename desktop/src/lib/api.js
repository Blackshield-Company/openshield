import { invoke, Channel } from '@tauri-apps/api/core';

/** Locate the openshark binary + version → { found, path, version } */
export async function getBinaryInfo() {
  const info = await invoke('binary_info');
  // version comes back as "openshark 1.1.0" — strip the name prefix
  if (info.version) info.version = info.version.replace(/^openshark\s+/i, '');
  return info;
}

/**
 * Run an openshark command and wait for full output.
 * Returns stdout as a string; throws with stderr if the command failed.
 */
export async function runCommand(args) {
  const result = await invoke('run_openshark', { args });
  if (result.code !== 0 && !result.stdout) {
    throw new Error(result.stderr || `openshark exited with code ${result.code}`);
  }
  // Some commands write useful info to stderr (progress etc.) — append if stdout is thin
  return result.stdout || result.stderr;
}

/**
 * Run an openshark command with streaming output.
 * onEvent receives { event: 'stdout'|'stderr'|'done', data }
 * (Rust sends { kind: 'line'|'error'|'done', text, code } — translated here.)
 */
export async function runStream(args, onEvent) {
  const channel = new Channel();
  channel.onmessage = (msg) => {
    switch (msg.kind) {
      case 'line':
        onEvent({ event: 'stdout', data: msg.text + '\n' });
        break;
      case 'error':
        onEvent({ event: 'stderr', data: msg.text + '\n' });
        break;
      case 'done':
        onEvent({ event: 'done', data: String(msg.code ?? 0) });
        break;
    }
  };
  return invoke('stream_openshark', { args, onEvent: channel });
}

/** Read config.toml → { path, content } */
export async function readConfig() {
  return invoke('read_config');
}

/** Write config.toml (backs up first) → { path, content } */
export async function writeConfig(content) {
  return invoke('write_config', { content });
}

/** Start (or adopt) an openshark serve API server → { running, owned, port, version }
 *  bindLan=true binds 0.0.0.0 (Tailscale/LAN access from the Android app) */
export async function serverStart(port, bindLan) {
  const args = {};
  if (port) args.port = port;
  if (bindLan) args.bindLan = true;
  return invoke('server_start', args);
}

/** Stop the server if we own it */
export async function serverStop() {
  return invoke('server_stop');
}

/** Server status without side effects → { running, owned, port, version } */
export async function serverStatus(port) {
  return invoke('server_status', port ? { port } : {});
}
