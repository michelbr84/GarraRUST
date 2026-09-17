// Utilitarios comuns dos testes. Nenhum teste deste diretorio abre socket de
// rede: o `makeWASocket` real e sempre substituido por `fakeSocket()`.
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';

export const BRIDGE_DIR = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
export const BRIDGE_PATH = path.join(BRIDGE_DIR, 'bridge.mjs');

/** Socket Baileys de mentira: guarda os handlers e deixa o teste dispara-los. */
export function fakeSocket(overrides = {}) {
  const handlers = new Map();
  const sock = {
    user: { id: '5511999990000:1@s.whatsapp.net', name: 'Garra Tester' },
    calls: { sendMessage: [], readMessages: [], presence: [], logout: 0, end: 0 },
    ev: {
      on(event, handler) {
        if (!handlers.has(event)) handlers.set(event, []);
        handlers.get(event).push(handler);
      },
    },
    fire(event, payload) {
      for (const handler of handlers.get(event) ?? []) handler(payload);
    },
    async sendMessage(jid, content) {
      sock.calls.sendMessage.push([jid, content]);
      return { key: { id: 'FAKEMSGID' } };
    },
    async readMessages(keys) {
      sock.calls.readMessages.push(keys);
    },
    async sendPresenceUpdate(type, jid) {
      sock.calls.presence.push([type, jid]);
    },
    async logout() {
      sock.calls.logout += 1;
    },
    async end() {
      sock.calls.end += 1;
    },
    ...overrides,
  };
  return sock;
}

/** Roda `node bridge.mjs` de verdade e devolve stdout/stderr/exit code. */
export function runBridge(args, stdin, { timeoutMs = 10_000 } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [BRIDGE_PATH, ...args], { cwd: BRIDGE_DIR });
    let stdout = '';
    let stderr = '';
    const timer = setTimeout(() => {
      child.kill('SIGKILL');
      reject(new Error('bridge did not exit in time'));
    }, timeoutMs);
    child.stdout.on('data', (d) => {
      stdout += d;
    });
    child.stderr.on('data', (d) => {
      stderr += d;
    });
    child.on('error', reject);
    child.on('close', (code) => {
      clearTimeout(timer);
      resolve({ code, stdout, stderr, events: parseEvents(stdout) });
    });
    if (stdin !== undefined) child.stdin.end(stdin);
  });
}

export function parseEvents(stdout) {
  return stdout
    .split('\n')
    .filter((l) => l.trim() !== '')
    .map((l) => JSON.parse(l));
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Espera ate `predicate(events)` ser verdade, ou falha por timeout. */
export async function waitFor(events, predicate, label, timeoutMs = 2000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate(events)) return;
    await sleep(5);
  }
  throw new Error(`timeout waiting for ${label}; got ${JSON.stringify(events)}`);
}
