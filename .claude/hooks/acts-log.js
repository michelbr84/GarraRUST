#!/usr/bin/env node
// Stop-hook helper: persists a per-session observation in the local
// awesome-claude-token-stack store (.acts/store.db) via MCP stdio.
// Fail-soft: any error is swallowed so the stop hook does not break.

import { spawn } from 'node:child_process';

const ACTS_MCP_BIN = process.env.ACTS_MCP_BIN
  || 'C:/Users/miche/tools/acts/packages/mcp/dist/bin/acts-mcp.js';
const ACTS_DB_PATH = process.env.ACTS_DB_PATH
  || 'G:/Projetos/GarraRUST/.acts/store.db';

const [branch = 'unknown', timestamp = new Date().toISOString(), ...rest] = process.argv.slice(2);
const summary = rest.join(' ') || 'no-summary';

const child = spawn('node', [ACTS_MCP_BIN], {
  env: { ...process.env, ACTS_DB_PATH },
  stdio: ['pipe', 'pipe', 'pipe'],
});

let stdoutBuf = '';
child.stdout.on('data', (d) => { stdoutBuf += d.toString(); });
child.stderr.on('data', () => { /* swallow */ });
child.on('error', () => { process.exit(0); });

const init = {
  jsonrpc: '2.0', id: 1, method: 'initialize',
  params: {
    protocolVersion: '2025-06-18',
    capabilities: {},
    clientInfo: { name: 'garraia-stop-hook', version: '1' },
  },
};
const save = {
  jsonrpc: '2.0', id: 2, method: 'tools/call',
  params: {
    name: 'acts_memory_save',
    arguments: {
      kind: 'note',
      title: `Session stop ${timestamp}`,
      body: `Branch: ${branch}\nSummary: ${summary}`,
      tags: ['session:stop', 'plan:foamy-origami', 'auto'],
      source: `stop-hook@${timestamp}`,
    },
  },
};

try {
  child.stdin.write(JSON.stringify(init) + '\n');
  child.stdin.write(JSON.stringify(save) + '\n');
} catch {
  process.exit(0);
}

setTimeout(() => {
  try { child.kill(); } catch { /* noop */ }
  const stored = stdoutBuf.includes('"id"') && stdoutBuf.includes('"kind"');
  if (stored) {
    process.stderr.write(`[acts-log] observation stored for session ${timestamp}\n`);
  }
  process.exit(0);
}, 2500);
