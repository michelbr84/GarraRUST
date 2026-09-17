#!/usr/bin/env node
// Valida um fluxo NDJSON de eventos bridge -> Rust contra o protocolo v1.
//
// Le stdin, escreve o veredito no stderr e sai 0 (valido) ou 1 (invalido).
// E o mesmo lint para a ponte real (`node bridge.mjs --protocol-check`) e para
// a fixture Python usada pelos testes do Rust: se as duas passam aqui, as duas
// falam o mesmo protocolo.

import { MAX_LINE_BYTES } from '../bridge.mjs';

const str = (v) => typeof v === 'string';
const optStr = (v) => v === null || v === undefined || typeof v === 'string';
const int = (v) => Number.isInteger(v);
const bool = (v) => typeof v === 'boolean';
const oneOf = (...values) => (v) => values.includes(v);

/** campo -> predicado. Campo ausente so passa se o predicado aceitar undefined. */
const SCHEMA = {
  started: {
    protocol: (v) => v === 1,
    bridge_version: str,
    baileys_version: optStr, // null = dependencias nao instaladas
    node_version: str,
  },
  qr: { data: str, expires_in_secs: int, attempt: (v) => int(v) && v >= 1 },
  status: {
    state: oneOf(
      'connecting',
      'waiting_scan',
      'authenticated',
      'syncing',
      'connected',
      'reconnecting',
      'disconnected',
    ),
    detail: optStr,
  },
  authenticated: {},
  connected: { jid: optStr, phone_last4: optStr, pushname: optStr },
  disconnected: {
    reason_code: int,
    reason: oneOf('restart_required', 'network', 'timeout', 'logged_out', 'replaced', 'unknown'),
    will_retry: bool,
    retry_in_ms: int,
  },
  logged_out: {},
  session_update: { session: str, seq: (v) => int(v) && v >= 1 },
  message: {
    id: str,
    chat_jid: str,
    sender_jid: str,
    sender_phone: optStr,
    text: optStr,
    media_kind: optStr,
    timestamp: int,
    is_group: bool,
    from_me: bool,
    push_name: optStr,
  },
  sent: { request_id: optStr, id: optStr },
  error: {
    request_id: optStr,
    code: oneOf('bad_request', 'not_connected', 'send_failed', 'protocol', 'internal'),
    message: str,
  },
  log: { level: oneOf('debug', 'info', 'warn', 'error'), message: str },
};

export function lintStream(text) {
  const errors = [];
  const lines = text.split('\n');
  let index = 0;
  let sawStarted = false;
  let lastSeq = 0;

  for (const line of lines) {
    if (line === '') continue;
    index += 1;
    const where = `line ${index}`;
    if (Buffer.byteLength(line, 'utf8') > MAX_LINE_BYTES) {
      errors.push(`${where}: exceeds ${MAX_LINE_BYTES} bytes`);
      continue;
    }
    let ev;
    try {
      ev = JSON.parse(line);
    } catch (err) {
      errors.push(`${where}: not valid JSON (${err.message})`);
      continue;
    }
    if (ev === null || typeof ev !== 'object' || Array.isArray(ev)) {
      errors.push(`${where}: not a JSON object`);
      continue;
    }
    const schema = SCHEMA[ev.type];
    if (!schema) {
      errors.push(`${where}: unknown event type ${JSON.stringify(ev.type)}`);
      continue;
    }
    if (index === 1 && ev.type !== 'started') {
      errors.push(`${where}: first event must be "started", got "${ev.type}"`);
    }
    if (ev.type === 'started') {
      if (sawStarted) errors.push(`${where}: duplicate "started"`);
      sawStarted = true;
    }
    for (const [field, predicate] of Object.entries(schema)) {
      if (!predicate(ev[field])) {
        errors.push(`${where}: field "${field}" invalid in "${ev.type}" (${JSON.stringify(ev[field])})`);
      }
    }
    for (const field of Object.keys(ev)) {
      if (field !== 'type' && !(field in schema)) {
        errors.push(`${where}: unexpected field "${field}" in "${ev.type}"`);
      }
    }
    if (ev.type === 'session_update') {
      if (ev.seq <= lastSeq) errors.push(`${where}: seq must increase (${lastSeq} -> ${ev.seq})`);
      lastSeq = ev.seq;
      try {
        JSON.parse(Buffer.from(ev.session, 'base64').toString('utf8'));
      } catch {
        errors.push(`${where}: session is not base64(JSON)`);
      }
    }
  }
  if (index === 0) errors.push('empty stream: expected at least a "started" event');
  else if (!sawStarted) errors.push('no "started" event in stream');
  return { count: index, errors };
}

async function readStdin() {
  const chunks = [];
  for await (const chunk of process.stdin) chunks.push(chunk);
  return Buffer.concat(chunks).toString('utf8');
}

const invokedDirectly = process.argv[1]?.endsWith('protocol-lint.mjs');
if (invokedDirectly) {
  const { count, errors } = lintStream(await readStdin());
  for (const err of errors) process.stderr.write(`protocol-lint: ${err}\n`);
  process.stderr.write(
    errors.length === 0
      ? `protocol-lint: OK (${count} events)\n`
      : `protocol-lint: ${errors.length} error(s) in ${count} events\n`,
  );
  process.exit(errors.length === 0 ? 0 : 1);
}
