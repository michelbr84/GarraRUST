#!/usr/bin/env node
// GarraIA — ponte WhatsApp (Node + Baileys), protocolo NDJSON v1 sobre stdio.
//
// Invariantes que este arquivo existe para sustentar:
//   - stdout e EXCLUSIVAMENTE NDJSON. Nenhum banner, nenhum QR desenhado,
//     nenhum log solto. Diagnostico sai como evento `log`, ja redigido.
//   - o bridge NUNCA escreve no disco. O estado de autenticacao vive em
//     memoria e volta para o Rust por `session_update`, que cifra e grava.
//   - material de autenticacao nunca entra num evento `log`.
//
// O contrato completo esta em docs/adr/0023-*.md e no README deste diretorio.

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

export const PROTOCOL_VERSION = 1;
export const BRIDGE_VERSION = '1.0.0';

/** Linha maior que isto e erro de framing: fecha a conexao com codigo 3. */
export const MAX_LINE_BYTES = 256 * 1024;
/** Debounce do `session_update` depois de qualquer mudanca em creds/keys. */
export const SESSION_DEBOUNCE_MS = 500;
/** Janela de silencio que encerra o modo `pair`, e o teto absoluto dela. */
export const PAIR_QUIET_MS = 2000;
export const PAIR_MAX_WAIT_MS = 10_000;
/** Validade nominal de um QR do Baileys (o Rust usa para o contador). */
export const QR_EXPIRES_SECS = 20;
/**
 * Teto para o `sock.end()`/`sock.logout()` do Baileys no caminho de saida.
 * Sem ele a saida trava: um socket que ainda esta ABRINDO nunca resolve o
 * `end()`, e ficamos sem emitir a `session_update` final — que e justamente a
 * unica coisa que o Rust nao pode perder.
 */
export const TEARDOWN_TIMEOUT_MS = 2000;

export const EXIT_OK = 0;
export const EXIT_FATAL = 1;
export const EXIT_LOGGED_OUT = 2;
export const EXIT_PROTOCOL = 3;

// ---------------------------------------------------------------------------
// Baileys e carregado sob demanda: `--protocol-check` precisa funcionar num
// checkout sem `npm ci`, e e exatamente esse o caso que o doctor do Rust quer
// diagnosticar.
// ---------------------------------------------------------------------------

let baileysPromise = null;
export function loadBaileys() {
  if (!baileysPromise) baileysPromise = import('@whiskeysockets/baileys');
  return baileysPromise;
}

export function readBaileysVersion() {
  const require = createRequire(import.meta.url);
  const candidates = [];
  try {
    candidates.push(require.resolve('@whiskeysockets/baileys/package.json'));
  } catch {
    /* pacote ausente: tentamos o caminho literal abaixo */
  }
  candidates.push(
    path.join(
      path.dirname(fileURLToPath(import.meta.url)),
      'node_modules/@whiskeysockets/baileys/package.json',
    ),
  );
  for (const file of candidates) {
    try {
      return JSON.parse(fs.readFileSync(file, 'utf8')).version ?? null;
    } catch {
      /* proximo candidato */
    }
  }
  return null; // dependencias nao instaladas — o Rust trata como "rode npm ci"
}

// ---------------------------------------------------------------------------
// Redacao
// ---------------------------------------------------------------------------

/** `5511999990000:1@s.whatsapp.net` -> `***0000@s.whatsapp.net`. */
export function redactJid(jid) {
  if (typeof jid !== 'string' || !jid.includes('@')) return '***';
  const at = jid.indexOf('@');
  const domain = jid.slice(at);
  const digits = jid.slice(0, at).split(':')[0].replace(/\D/g, '');
  return `***${digits.slice(-4)}${domain}`;
}

/**
 * Mascara JIDs e qualquer corrida longa de digitos numa string livre. Usada em
 * TODO texto que vai para um evento `log` — inclusive mensagem de excecao, que
 * e o lugar por onde um JID cru costuma vazar.
 */
export function redact(text) {
  return String(text)
    .replace(/[0-9]{5,}(?::\d+)?@[a-z.]+/gi, (m) => redactJid(m))
    .replace(/\b\d{6,}\b/g, (m) => `***${m.slice(-4)}`);
}

/** So o formato do erro, nunca o objeto — um Boom carrega `data` arbitrario. */
export function describeError(err) {
  if (!err) return 'unknown error';
  const name = err.name || 'Error';
  return redact(`${name}: ${err.message ?? String(err)}`);
}

export function phoneLast4(jid) {
  if (typeof jid !== 'string') return null;
  const digits = jid.split('@')[0].split(':')[0].replace(/\D/g, '');
  return digits.length >= 4 ? digits.slice(-4) : null;
}

/**
 * E.164 a partir do JID, quando ele ja e um numero. `@lid` e um identificador
 * opaco: em v1 NAO tentamos resolver LID -> telefone (exigiria consultar o
 * mapa de LID do proprio Baileys, que muda entre versoes). Devolve null.
 */
export function senderPhone(jid) {
  if (typeof jid !== 'string') return null;
  const [user, domain] = jid.split('@');
  if (domain !== 's.whatsapp.net') return null;
  const digits = user.split(':')[0];
  return /^\d{6,15}$/.test(digits) ? `+${digits}` : null;
}

// ---------------------------------------------------------------------------
// Framing NDJSON
// ---------------------------------------------------------------------------

export class OversizeLineError extends Error {}

/**
 * Splitter de linhas com teto de bytes. Nao usamos `readline` porque ele nao
 * tem limite: uma linha SEM `\n` cresceria sem teto na memoria, e o teto de
 * 256 KiB do protocolo precisa valer tambem para entrada nao terminada.
 */
export class LineFramer {
  constructor(maxBytes = MAX_LINE_BYTES) {
    this.maxBytes = maxBytes;
    this.pending = Buffer.alloc(0);
  }

  /** @returns {string[]} linhas completas; lanca OversizeLineError no estouro */
  push(chunk) {
    this.pending = Buffer.concat([this.pending, Buffer.from(chunk)]);
    const lines = [];
    let nl;
    while ((nl = this.pending.indexOf(0x0a)) !== -1) {
      const raw = this.pending.subarray(0, nl);
      this.pending = this.pending.subarray(nl + 1);
      if (raw.length > this.maxBytes) throw new OversizeLineError('line too long');
      lines.push(raw.toString('utf8').replace(/\r$/, ''));
    }
    if (this.pending.length > this.maxBytes) throw new OversizeLineError('line too long');
    return lines;
  }
}

/**
 * @returns {{ok:true,value:object}|{ok:false,reason:string}}
 * Falha aqui e erro de FRAMING (encerra com codigo 3). Comando conhecido com
 * campo errado e `bad_request` e nao derruba o bridge — sao coisas diferentes.
 */
export function parseCommand(line) {
  let value;
  try {
    value = JSON.parse(line);
  } catch {
    return { ok: false, reason: 'line is not valid JSON' };
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return { ok: false, reason: 'line is not a JSON object' };
  }
  if (typeof value.type !== 'string' || value.type.length === 0) {
    return { ok: false, reason: 'missing "type" field' };
  }
  return { ok: true, value };
}

// ---------------------------------------------------------------------------
// Estado de autenticacao em memoria
// ---------------------------------------------------------------------------

/**
 * Mesmo contrato do `useMultiFileAuthState` oficial, mas sem tocar no disco.
 * `snapshot()` devolve `{creds, keys}` pronto para `BufferJSON.replacer`.
 */
export async function makeMemoryAuthState(initial, onChange = () => {}) {
  const { initAuthCreds, proto } = await loadBaileys();
  const creds = initial?.creds ?? initAuthCreds();
  const store = initial?.keys ?? {};

  return {
    state: {
      creds,
      keys: {
        get: async (type, ids) => {
          const bucket = store[type] ?? {};
          const out = {};
          for (const id of ids) {
            let value = bucket[id];
            // Mesma reidratacao do useMultiFileAuthState: o Baileys espera a
            // mensagem protobuf, nao o objeto plano que saiu do JSON.
            if (type === 'app-state-sync-key' && value) {
              value = proto.Message.AppStateSyncKeyData.fromObject(value);
            }
            out[id] = value;
          }
          return out;
        },
        set: async (data) => {
          for (const category of Object.keys(data)) {
            store[category] ??= {};
            for (const id of Object.keys(data[category])) {
              const value = data[category][id];
              if (value) store[category][id] = value;
              else delete store[category][id];
            }
            if (Object.keys(store[category]).length === 0) delete store[category];
          }
          onChange();
        },
      },
    },
    snapshot: () => ({ creds, keys: store }),
  };
}

export async function encodeSnapshot(snapshot) {
  const { BufferJSON } = await loadBaileys();
  return Buffer.from(JSON.stringify(snapshot, BufferJSON.replacer), 'utf8').toString('base64');
}

export async function decodeSnapshot(b64) {
  if (b64 === null || b64 === undefined) return null;
  const { BufferJSON } = await loadBaileys();
  const json = Buffer.from(String(b64), 'base64').toString('utf8');
  const value = JSON.parse(json, BufferJSON.reviver);
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('session snapshot is not an object');
  }
  return { creds: value.creds ?? null, keys: value.keys ?? {} };
}

// ---------------------------------------------------------------------------
// Queda de conexao
// ---------------------------------------------------------------------------

/**
 * `retry`: 'immediate' | 'backoff' | 'never'.
 * `exit`: codigo de saida quando `retry === 'never'`.
 *
 * 403/419 entram junto com o 401 porque sao os `UNAUTHORIZED_CODES` do proprio
 * Baileys: a sessao nao volta a funcionar, e insistir so gera loop. O Rust
 * trata os tres como "apague a sessao e peca novo pareamento".
 */
export function mapDisconnectReason(code) {
  switch (code) {
    case 515:
      return { reason: 'restart_required', retry: 'immediate' };
    case 401:
    case 403:
    case 419:
      return { reason: 'logged_out', retry: 'never', exit: EXIT_LOGGED_OUT };
    case 440:
      return { reason: 'replaced', retry: 'never', exit: EXIT_OK };
    case 408:
      return { reason: 'timeout', retry: 'backoff' };
    case 411:
    case 428:
    case 500:
    case 503:
      return { reason: 'network', retry: 'backoff' };
    default:
      return { reason: 'unknown', retry: 'backoff' };
  }
}

/** 1 s -> 30 s exponencial, com ate +25% de jitter, teto duro em 30 s. */
export function backoffDelayMs(attempt, rng = Math.random) {
  const base = Math.min(30_000, 1000 * 2 ** Math.max(0, attempt - 1));
  return Math.min(30_000, base + Math.floor(rng() * base * 0.25));
}

// ---------------------------------------------------------------------------
// Mensagens recebidas
// ---------------------------------------------------------------------------

const MEDIA_KINDS = {
  imageMessage: 'image',
  videoMessage: 'video',
  audioMessage: 'audio',
  documentMessage: 'document',
  stickerMessage: 'sticker',
  ptvMessage: 'video',
  locationMessage: 'location',
  liveLocationMessage: 'location',
  contactMessage: 'contact',
  contactsArrayMessage: 'contact',
};

/** Desembrulha os containers que o WhatsApp poe por fora do conteudo real. */
export function unwrapMessage(message) {
  let current = message;
  for (let depth = 0; current && depth < 4; depth += 1) {
    const inner =
      current.ephemeralMessage?.message ??
      current.viewOnceMessage?.message ??
      current.viewOnceMessageV2?.message ??
      current.viewOnceMessageV2Extension?.message ??
      current.documentWithCaptionMessage?.message ??
      current.editedMessage?.message;
    if (!inner) return current;
    current = inner;
  }
  return current;
}

/** @returns {object|null} evento `message`, ou null quando nao ha o que entregar */
export function toMessageEvent(waMessage, ownJid) {
  const key = waMessage?.key;
  const chatJid = key?.remoteJid;
  if (!key?.id || typeof chatJid !== 'string') return null;
  if (chatJid === 'status@broadcast' || chatJid.endsWith('@newsletter')) return null;

  const content = unwrapMessage(waMessage.message);
  if (!content) return null;

  const text = content.conversation ?? content.extendedTextMessage?.text ?? null;
  let mediaKind = null;
  for (const field of Object.keys(MEDIA_KINDS)) {
    if (content[field]) {
      mediaKind = MEDIA_KINDS[field];
      break;
    }
  }
  // Reacao, recibo, protocolMessage: nada que o agente possa responder.
  if (text === null && mediaKind === null) return null;

  const isGroup = chatJid.endsWith('@g.us');
  const fromMe = Boolean(key.fromMe);
  const senderJid = isGroup
    ? (key.participant ?? waMessage.participant ?? chatJid)
    : fromMe
      ? (ownJid ?? chatJid)
      : chatJid;
  const ts = waMessage.messageTimestamp;

  return {
    type: 'message',
    id: key.id,
    chat_jid: chatJid,
    sender_jid: senderJid,
    sender_phone: senderPhone(senderJid),
    text: mediaKind ? null : text,
    media_kind: mediaKind,
    timestamp: typeof ts === 'object' && ts !== null ? Number(ts.toNumber?.() ?? ts) : Number(ts ?? 0),
    is_group: isGroup,
    from_me: fromMe,
    push_name: waMessage.pushName ?? null,
  };
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

const defaultSocketFactory = async (options) => {
  const { makeWASocket } = await loadBaileys();
  return makeWASocket(options);
};

/**
 * @param {object} deps
 * @param {(ev:object)=>void} deps.emit          escreve UMA linha NDJSON
 * @param {(code:number)=>void} deps.exit        encerra o processo
 * @param {(o:object)=>Promise<object>} [deps.socketFactory] injetavel nos testes
 *
 * Os tres knobs de tempo existem para os testes: o comportamento em producao e
 * o do protocolo (500 ms de debounce, janela de 2 s, teto de 10 s).
 */
export function createBridge({
  emit,
  exit,
  socketFactory = defaultSocketFactory,
  rng = Math.random,
  sessionDebounceMs = SESSION_DEBOUNCE_MS,
  pairQuietMs = PAIR_QUIET_MS,
  pairMaxWaitMs = PAIR_MAX_WAIT_MS,
  teardownTimeoutMs = TEARDOWN_TIMEOUT_MS,
}) {
  const state = {
    mode: null,
    sessionLoaded: false,
    auth: null,
    sock: null,
    seq: 0,
    dirty: false,
    debounce: null,
    attempt: 0,
    qrAttempt: 0,
    stopping: false,
    announcedAuth: false,
    connectedAt: 0,
    lastChangeAt: 0,
    pairTimer: null,
  };

  const log = (level, message) => emit({ type: 'log', level, message: redact(message) });

  function markDirty() {
    state.dirty = true;
    state.lastChangeAt = Date.now();
    if (state.debounce) return;
    state.debounce = setTimeout(() => {
      state.debounce = null;
      void flushSession();
    }, sessionDebounceMs);
  }

  async function flushSession({ force = false } = {}) {
    if (state.debounce) {
      clearTimeout(state.debounce);
      state.debounce = null;
    }
    if (!state.auth) return;
    if (!state.dirty && !force) return;
    state.dirty = false;
    state.seq += 1;
    emit({ type: 'session_update', session: await encodeSnapshot(state.auth.snapshot()), seq: state.seq });
  }

  async function shutdown(code, { logout = false } = {}) {
    if (state.stopping) return;
    state.stopping = true;
    if (state.pairTimer) clearInterval(state.pairTimer);
    if (state.sock) {
      const teardown = Promise.resolve()
        .then(() => (logout ? state.sock.logout() : state.sock.end(undefined)))
        .catch((err) => log('debug', `socket teardown: ${describeError(err)}`));
      await Promise.race([
        teardown,
        new Promise((resolve) => {
          setTimeout(resolve, teardownTimeoutMs).unref();
        }),
      ]);
    }
    // A ultima `session_update` e sempre emitida, mesmo sem mudanca pendente:
    // o Rust precisa poder confiar nela como o estado final da sessao.
    await flushSession({ force: true });
    exit(code);
  }

  function scheduleReconnect(kind) {
    if (state.stopping || state.mode === null) return 0;
    const delay = kind === 'immediate' ? 0 : backoffDelayMs(state.attempt + 1, rng);
    if (kind !== 'immediate') state.attempt += 1;
    setTimeout(() => {
      if (state.stopping) return;
      connect().catch((err) => {
        emit({ type: 'error', code: 'internal', message: describeError(err) });
        void shutdown(EXIT_FATAL);
      });
    }, delay);
    return delay;
  }

  function armPairWatchdog() {
    if (state.mode !== 'pair' || state.pairTimer) return;
    state.connectedAt = Date.now();
    state.lastChangeAt = state.connectedAt;
    // O telefone ainda empurra chaves de app-state logo depois do pareamento.
    // Saimos na primeira janela de 2 s sem mudanca, com teto de 10 s.
    state.pairTimer = setInterval(() => {
      const now = Date.now();
      if (now - state.lastChangeAt >= pairQuietMs || now - state.connectedAt >= pairMaxWaitMs) {
        clearInterval(state.pairTimer);
        state.pairTimer = null;
        void shutdown(EXIT_OK);
      }
    }, Math.max(10, Math.floor(pairQuietMs / 8)));
  }

  function onConnectionUpdate(update) {
    if (state.stopping) return;
    const { connection, lastDisconnect, qr } = update;

    if (qr) {
      state.qrAttempt += 1;
      emit({ type: 'qr', data: qr, expires_in_secs: QR_EXPIRES_SECS, attempt: state.qrAttempt });
      emit({ type: 'status', state: 'waiting_scan', detail: `qr attempt ${state.qrAttempt}` });
    }

    if (update.isNewLogin) {
      emit({ type: 'status', state: 'syncing', detail: 'app state sync' });
    }

    if (connection === 'open') {
      state.attempt = 0;
      const jid = state.sock?.user?.id ?? null;
      emit({ type: 'status', state: 'connected', detail: '' });
      emit({
        type: 'connected',
        jid,
        phone_last4: phoneLast4(jid),
        pushname: state.sock?.user?.name ?? null,
      });
      markDirty();
      armPairWatchdog();
    }

    if (connection === 'close') {
      const code = lastDisconnect?.error?.output?.statusCode ?? lastDisconnect?.error?.statusCode ?? 0;
      const mapped = mapDisconnectReason(code);
      const willRetry = mapped.retry !== 'never' && !state.stopping;
      const retryIn = willRetry ? scheduleReconnect(mapped.retry) : 0;
      emit({
        type: 'disconnected',
        reason_code: code,
        reason: mapped.reason,
        will_retry: willRetry,
        retry_in_ms: retryIn,
      });
      emit({ type: 'status', state: willRetry ? 'reconnecting' : 'disconnected', detail: mapped.reason });
      if (mapped.reason === 'logged_out') {
        emit({ type: 'logged_out' });
      }
      if (!willRetry) void shutdown(mapped.exit ?? EXIT_OK);
    }
  }

  async function connect() {
    emit({ type: 'status', state: state.attempt > 0 ? 'reconnecting' : 'connecting', detail: '' });
    const sock = await socketFactory({
      auth: state.auth.state,
      // No Baileys 7 a opcao ja nem existe; fica explicita porque e o
      // invariante do projeto — quem desenha o QR e o Rust, nunca o bridge.
      printQRInTerminal: false,
      logger: await makeSilentLogger(),
      browser: ['GarraIA', 'Desktop', BRIDGE_VERSION],
      markOnlineOnConnect: false,
      // O telefone continua recebendo notificacao; e um linked device, nao um
      // substituto. Historico completo ficaria so ocupando memoria do bridge.
      syncFullHistory: false,
    });
    state.sock = sock;
    sock.ev.on('connection.update', onConnectionUpdate);
    sock.ev.on('creds.update', () => {
      if (state.auth.snapshot().creds?.me || state.auth.snapshot().creds?.registered) {
        if (!state.announcedAuth) {
          state.announcedAuth = true;
          emit({ type: 'status', state: 'authenticated', detail: '' });
          emit({ type: 'authenticated' });
        }
      }
      markDirty();
    });
    if (state.mode === 'serve') {
      sock.ev.on('messages.upsert', ({ messages, type }) => {
        if (type !== 'notify') return;
        for (const waMessage of messages ?? []) {
          const ev = toMessageEvent(waMessage, state.sock?.user?.id ?? null);
          if (ev) emit(ev);
        }
      });
    }
  }

  async function handleCommand(cmd) {
    if (cmd.type !== 'session_load' && !state.sessionLoaded) {
      emit({ type: 'error', code: 'bad_request', message: 'session_load must be the first command' });
      return;
    }
    switch (cmd.type) {
      case 'session_load': {
        if (state.sessionLoaded) {
          emit({ type: 'error', code: 'bad_request', message: 'session already loaded' });
          return;
        }
        let restored = null;
        try {
          restored = await decodeSnapshot(cmd.session ?? null);
        } catch (err) {
          emit({ type: 'error', code: 'bad_request', message: `invalid session blob: ${describeError(err)}` });
          return;
        }
        state.auth = await makeMemoryAuthState(restored?.creds ? restored : null, markDirty);
        state.sessionLoaded = true;
        log('info', restored?.creds ? 'session restored' : 'no session: pairing required');
        return;
      }
      case 'start': {
        if (state.mode) {
          emit({ type: 'error', code: 'bad_request', message: 'already started' });
          return;
        }
        if (cmd.mode !== 'pair' && cmd.mode !== 'serve') {
          emit({ type: 'error', code: 'bad_request', message: 'mode must be "pair" or "serve"' });
          return;
        }
        state.mode = cmd.mode;
        try {
          await connect();
        } catch (err) {
          emit({ type: 'error', code: 'internal', message: describeError(err) });
          await shutdown(EXIT_FATAL);
        }
        return;
      }
      case 'send': {
        const { request_id: requestId, chat_jid: chatJid, text } = cmd;
        if (typeof chatJid !== 'string' || typeof text !== 'string') {
          emit({ type: 'error', request_id: requestId, code: 'bad_request', message: 'chat_jid and text are required' });
          return;
        }
        if (!state.sock || !state.sock.user) {
          emit({ type: 'error', request_id: requestId, code: 'not_connected', message: 'socket is not connected' });
          return;
        }
        try {
          const res = await state.sock.sendMessage(chatJid, { text });
          emit({ type: 'sent', request_id: requestId, id: res?.key?.id ?? null });
        } catch (err) {
          emit({ type: 'error', request_id: requestId, code: 'send_failed', message: describeError(err) });
        }
        return;
      }
      case 'read': {
        const ids = Array.isArray(cmd.ids) ? cmd.ids : [];
        if (typeof cmd.chat_jid !== 'string' || ids.length === 0) {
          emit({ type: 'error', code: 'bad_request', message: 'chat_jid and non-empty ids are required' });
          return;
        }
        try {
          await state.sock?.readMessages(ids.map((id) => ({ remoteJid: cmd.chat_jid, id, fromMe: false })));
        } catch (err) {
          log('warn', `readMessages failed: ${describeError(err)}`);
        }
        return;
      }
      case 'typing': {
        if (typeof cmd.chat_jid !== 'string') {
          emit({ type: 'error', code: 'bad_request', message: 'chat_jid is required' });
          return;
        }
        try {
          await state.sock?.sendPresenceUpdate(cmd.on ? 'composing' : 'paused', cmd.chat_jid);
        } catch (err) {
          log('warn', `sendPresenceUpdate failed: ${describeError(err)}`);
        }
        return;
      }
      case 'logout':
        emit({ type: 'logged_out' });
        await shutdown(EXIT_OK, { logout: true });
        return;
      case 'shutdown':
        await shutdown(EXIT_OK);
        return;
      default:
        emit({ type: 'error', code: 'bad_request', message: `unknown command "${cmd.type}"` });
    }
  }

  return { handleCommand, shutdown, flushSession, state };
}

/**
 * Logger do Baileys. Duas camadas de protecao do stdout: nivel `silent` (nada
 * e emitido) e destino fd 2 (se alguma versao futura subir o nivel sozinha, o
 * texto sai no stderr e nao contamina o NDJSON).
 */
async function makeSilentLogger() {
  try {
    const { default: pino } = await import('pino');
    return pino({ level: 'silent' }, pino.destination({ dest: 2, sync: true }));
  } catch {
    const noop = () => {};
    const logger = { level: 'silent', trace: noop, debug: noop, info: noop, warn: noop, error: noop, fatal: noop };
    logger.child = () => logger;
    return logger;
  }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

export function startedEvent() {
  return {
    type: 'started',
    protocol: PROTOCOL_VERSION,
    bridge_version: BRIDGE_VERSION,
    baileys_version: readBaileysVersion(),
    node_version: process.versions.node,
  };
}

async function main(argv) {
  const emit = (ev) => {
    try {
      process.stdout.write(`${JSON.stringify(ev)}\n`);
    } catch {
      /* stdout fechado: o Rust sumiu, nao ha para quem reportar */
    }
  };
  const exit = (code) => {
    // `process.exit` trunca stdout quando ele e um pipe; o write vazio so
    // resolve depois que a fila drenou.
    process.exitCode = code;
    setTimeout(() => process.exit(code), 500).unref();
    process.stdout.write('', () => process.exit(code));
  };

  emit(startedEvent());
  if (argv.includes('--protocol-check')) {
    exit(EXIT_OK);
    return;
  }

  const bridge = createBridge({ emit, exit });

  process.on('uncaughtException', (err) => {
    emit({ type: 'error', code: 'internal', message: describeError(err) });
    exit(EXIT_FATAL);
  });
  process.on('unhandledRejection', (err) => {
    emit({ type: 'error', code: 'internal', message: describeError(err) });
    exit(EXIT_FATAL);
  });
  for (const signal of ['SIGTERM', 'SIGINT']) {
    process.on(signal, () => void bridge.shutdown(EXIT_OK));
  }

  const framer = new LineFramer();
  let queue = Promise.resolve();
  process.stdin.on('data', (chunk) => {
    let lines;
    try {
      lines = framer.push(chunk);
    } catch (err) {
      emit({ type: 'error', code: 'protocol', message: describeError(err) });
      exit(EXIT_PROTOCOL);
      return;
    }
    for (const line of lines) {
      if (line.trim() === '') continue;
      const parsed = parseCommand(line);
      if (!parsed.ok) {
        emit({ type: 'error', code: 'protocol', message: parsed.reason });
        exit(EXIT_PROTOCOL);
        return;
      }
      queue = queue.then(() => bridge.handleCommand(parsed.value));
    }
  });
  process.stdin.on('end', () => {
    queue = queue.then(() => bridge.shutdown(EXIT_OK));
  });
}

const invokedDirectly =
  process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) {
  main(process.argv.slice(2)).catch((err) => {
    process.stdout.write(`${JSON.stringify({ type: 'error', code: 'internal', message: describeError(err) })}\n`);
    process.exitCode = EXIT_FATAL;
  });
}
