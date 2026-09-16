// Fluxo completo com o socket do Baileys injetado: nenhum teste aqui abre
// conexao de rede nem depende de telefone.
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createBridge,
  decodeSnapshot,
  EXIT_OK,
  EXIT_LOGGED_OUT,
} from '../bridge.mjs';
import { fakeSocket, waitFor, sleep } from './helpers.mjs';

/** Monta um bridge com tempos curtos e um socket controlado pelo teste. */
function harness(options = {}) {
  const events = [];
  const exits = [];
  const sockets = [];
  const bridge = createBridge({
    emit: (ev) => events.push(ev),
    exit: (code) => exits.push(code),
    socketFactory: async (opts) => {
      const sock = fakeSocket();
      sock.options = opts;
      sockets.push(sock);
      return sock;
    },
    rng: () => 0,
    sessionDebounceMs: 10,
    pairQuietMs: 60,
    pairMaxWaitMs: 400,
    ...options,
  });
  const of = (type) => events.filter((e) => e.type === type);
  return { bridge, events, exits, sockets, of, last: () => sockets.at(-1) };
}

async function started(h, mode = 'serve') {
  await h.bridge.handleCommand({ type: 'session_load', session: null });
  await h.bridge.handleCommand({ type: 'start', mode });
  return h.last();
}

test('opcoes do socket travam os invariantes do projeto', async () => {
  const h = harness();
  const sock = await started(h, 'pair');
  assert.equal(sock.options.printQRInTerminal, false);
  assert.equal(sock.options.markOnlineOnConnect, false);
  assert.equal(sock.options.syncFullHistory, false);
  assert.deepEqual(sock.options.browser.slice(0, 2), ['GarraIA', 'Desktop']);
  assert.equal(sock.options.logger.level, 'silent');
  await h.bridge.shutdown(EXIT_OK);
});

test('QR: um evento por tentativa, com contador crescente', async () => {
  const h = harness();
  const sock = await started(h, 'pair');
  sock.fire('connection.update', { qr: 'QR-A' });
  sock.fire('connection.update', { qr: 'QR-B' });
  assert.deepEqual(
    h.of('qr').map((e) => [e.data, e.attempt, e.expires_in_secs]),
    [
      ['QR-A', 1, 20],
      ['QR-B', 2, 20],
    ],
  );
  assert.deepEqual(h.of('status').at(-1), {
    type: 'status',
    state: 'waiting_scan',
    detail: 'qr attempt 2',
  });
  await h.bridge.shutdown(EXIT_OK);
});

test('connected traz jid, ultimos 4 digitos e pushname', async () => {
  const h = harness();
  const sock = await started(h);
  sock.fire('connection.update', { connection: 'open' });
  assert.deepEqual(h.of('connected'), [
    {
      type: 'connected',
      jid: '5511999990000:1@s.whatsapp.net',
      phone_last4: '0000',
      pushname: 'Garra Tester',
    },
  ]);
  await h.bridge.shutdown(EXIT_OK);
});

test('modo pair sai 0 apos a janela de silencio, com session_update final', async () => {
  const h = harness();
  const sock = await started(h, 'pair');
  sock.fire('connection.update', { connection: 'open' });
  await waitFor(h.exits, (e) => e.length > 0, 'exit do modo pair');
  assert.deepEqual(h.exits, [EXIT_OK]);
  const updates = h.of('session_update');
  assert.ok(updates.length >= 1);
  assert.deepEqual(
    updates.map((u) => u.seq),
    updates.map((_, i) => i + 1),
    'seq deve crescer de 1 em 1',
  );
  const restored = await decodeSnapshot(updates.at(-1).session);
  assert.ok(restored.creds.noiseKey, 'o snapshot final tem de ser restauravel');
});

test('515 reconecta imediatamente; um socket novo e criado', async () => {
  const h = harness();
  const sock = await started(h);
  sock.fire('connection.update', {
    connection: 'close',
    lastDisconnect: { error: { output: { statusCode: 515 } } },
  });
  assert.deepEqual(h.of('disconnected'), [
    {
      type: 'disconnected',
      reason_code: 515,
      reason: 'restart_required',
      will_retry: true,
      retry_in_ms: 0,
    },
  ]);
  await waitFor(h.sockets, (s) => s.length === 2, 'reconexao imediata');
  await h.bridge.shutdown(EXIT_OK);
});

test('queda de rede agenda backoff e anuncia o retry_in_ms', async () => {
  const h = harness();
  const sock = await started(h);
  sock.fire('connection.update', {
    connection: 'close',
    lastDisconnect: { error: { output: { statusCode: 428 } } },
  });
  const ev = h.of('disconnected')[0];
  assert.equal(ev.reason, 'network');
  assert.equal(ev.will_retry, true);
  assert.equal(ev.retry_in_ms, 1000); // rng fixado em 0 -> sem jitter
  await h.bridge.shutdown(EXIT_OK); // cancela a reconexao pendente
});

test('logged_out: emite o evento e sai com codigo 2', async () => {
  const h = harness();
  const sock = await started(h);
  sock.fire('connection.update', {
    connection: 'close',
    lastDisconnect: { error: { output: { statusCode: 401 } } },
  });
  await waitFor(h.exits, (e) => e.length > 0, 'saida por logout');
  assert.equal(h.of('disconnected')[0].will_retry, false);
  assert.equal(h.of('logged_out').length, 1);
  assert.deepEqual(h.exits, [EXIT_LOGGED_OUT]);
  assert.equal(h.sockets.length, 1, 'nao pode tentar reconectar');
});

test('connectionReplaced nao reconecta e sai 0', async () => {
  const h = harness();
  const sock = await started(h);
  sock.fire('connection.update', {
    connection: 'close',
    lastDisconnect: { error: { output: { statusCode: 440 } } },
  });
  await waitFor(h.exits, (e) => e.length > 0, 'saida por replaced');
  assert.equal(h.of('disconnected')[0].reason, 'replaced');
  assert.deepEqual(h.exits, [EXIT_OK]);
  assert.equal(h.of('logged_out').length, 0);
});

test('send responde sent; sem socket responde not_connected', async () => {
  const h = harness();
  const sock = await started(h);
  sock.fire('connection.update', { connection: 'open' });
  await h.bridge.handleCommand({
    type: 'send',
    request_id: 'req-1',
    chat_jid: '5511888880000@s.whatsapp.net',
    text: 'ola',
  });
  assert.deepEqual(h.of('sent'), [{ type: 'sent', request_id: 'req-1', id: 'FAKEMSGID' }]);
  assert.deepEqual(sock.calls.sendMessage, [['5511888880000@s.whatsapp.net', { text: 'ola' }]]);

  await h.bridge.handleCommand({ type: 'send', request_id: 'req-2', chat_jid: 1, text: 'x' });
  assert.equal(h.of('error').at(-1).code, 'bad_request');
  await h.bridge.shutdown(EXIT_OK);
});

test('send com socket caido responde not_connected, nao send_failed', async () => {
  const h = harness();
  await started(h);
  h.bridge.state.sock.user = undefined;
  await h.bridge.handleCommand({
    type: 'send',
    request_id: 'r',
    chat_jid: '5511888880000@s.whatsapp.net',
    text: 'x',
  });
  assert.equal(h.of('error').at(-1).code, 'not_connected');
  await h.bridge.shutdown(EXIT_OK);
});

test('read e typing sao best-effort e nao emitem resposta', async () => {
  const h = harness();
  const sock = await started(h);
  sock.fire('connection.update', { connection: 'open' });
  const before = h.events.length;
  await h.bridge.handleCommand({ type: 'read', chat_jid: 'a@s.whatsapp.net', ids: ['X'] });
  await h.bridge.handleCommand({ type: 'typing', chat_jid: 'a@s.whatsapp.net', on: true });
  await h.bridge.handleCommand({ type: 'typing', chat_jid: 'a@s.whatsapp.net', on: false });
  assert.equal(h.events.length, before, 'nenhum evento novo');
  assert.deepEqual(sock.calls.readMessages, [[{ remoteJid: 'a@s.whatsapp.net', id: 'X', fromMe: false }]]);
  assert.deepEqual(sock.calls.presence, [
    ['composing', 'a@s.whatsapp.net'],
    ['paused', 'a@s.whatsapp.net'],
  ]);
  await h.bridge.shutdown(EXIT_OK);
});

test('serve entrega messages.upsert type notify e ignora o resto', async () => {
  const h = harness();
  const sock = await started(h, 'serve');
  sock.fire('connection.update', { connection: 'open' });
  const upsert = {
    messages: [
      { key: { id: 'A', remoteJid: '5511888880000@s.whatsapp.net' }, message: { conversation: 'oi' }, messageTimestamp: 1 },
      { key: { id: 'B', remoteJid: 'status@broadcast' }, message: { conversation: 'ignorar' }, messageTimestamp: 2 },
    ],
  };
  sock.fire('messages.upsert', { ...upsert, type: 'append' });
  assert.equal(h.of('message').length, 0, 'type append nao e entregue');
  sock.fire('messages.upsert', { ...upsert, type: 'notify' });
  assert.deepEqual(h.of('message').map((m) => m.id), ['A']);
  await h.bridge.shutdown(EXIT_OK);
});

test('pair nao assina messages.upsert', async () => {
  const h = harness();
  const sock = await started(h, 'pair');
  sock.fire('messages.upsert', {
    type: 'notify',
    messages: [{ key: { id: 'A', remoteJid: 'x@s.whatsapp.net' }, message: { conversation: 'oi' }, messageTimestamp: 1 }],
  });
  assert.equal(h.of('message').length, 0);
  await h.bridge.shutdown(EXIT_OK);
});

test('creds.update anuncia authenticated uma unica vez', async () => {
  const h = harness();
  const sock = await started(h);
  h.bridge.state.auth.snapshot().creds.registered = true;
  sock.fire('creds.update', {});
  sock.fire('creds.update', {});
  assert.equal(h.of('authenticated').length, 1);
  await h.bridge.shutdown(EXIT_OK);
});

test('session_update e debounced: varias mudancas, um evento so', async () => {
  const h = harness();
  const sock = await started(h);
  for (let i = 0; i < 5; i += 1) sock.fire('creds.update', {});
  await sleep(40);
  assert.equal(h.of('session_update').length, 1);
  assert.equal(h.of('session_update')[0].seq, 1);
  await h.bridge.shutdown(EXIT_OK);
});

test('shutdown fecha o socket sem deslogar e flusha a sessao final', async () => {
  const h = harness();
  const sock = await started(h);
  await h.bridge.handleCommand({ type: 'shutdown' });
  assert.equal(sock.calls.end, 1);
  assert.equal(sock.calls.logout, 0);
  assert.equal(h.of('session_update').length, 1, 'flush final acontece mesmo sem mudanca pendente');
  assert.deepEqual(h.exits, [EXIT_OK]);
});

test('teardown travado nao segura a session_update final', async () => {
  // O `end()` do Baileys num socket que ainda esta ABRINDO pode nunca
  // resolver. A ultima `session_update` e a unica coisa que o Rust nao pode
  // perder, entao a saida corre contra um timeout em vez de esperar o socket.
  const h = harness({ teardownTimeoutMs: 30 });
  await h.bridge.handleCommand({ type: 'session_load', session: null });
  await h.bridge.handleCommand({ type: 'start', mode: 'serve' });
  h.last().end = () => new Promise(() => {});
  await h.bridge.handleCommand({ type: 'shutdown' });
  assert.equal(h.of('session_update').length, 1);
  assert.deepEqual(h.exits, [EXIT_OK]);
});

test('logout desloga, emite logged_out e sai 0', async () => {
  const h = harness();
  const sock = await started(h);
  await h.bridge.handleCommand({ type: 'logout' });
  assert.equal(sock.calls.logout, 1);
  assert.equal(h.of('logged_out').length, 1, 'o logout pedido nao pode duplicar o evento');
  assert.deepEqual(h.exits, [EXIT_OK]);
});

test('session_load restaura a sessao e so aceita uma vez', async () => {
  const h = harness();
  await h.bridge.handleCommand({ type: 'session_load', session: null });
  await h.bridge.handleCommand({ type: 'session_load', session: null });
  assert.equal(h.of('error').at(-1).code, 'bad_request');

  const h2 = harness();
  const blob = Buffer.from(
    JSON.stringify({ creds: { me: { id: '5511999990000:1@s.whatsapp.net' } }, keys: {} }),
  ).toString('base64');
  await h2.bridge.handleCommand({ type: 'session_load', session: blob });
  assert.deepEqual(h2.bridge.state.auth.snapshot().creds.me, {
    id: '5511999990000:1@s.whatsapp.net',
  });
  assert.equal(h2.of('log').at(-1).message, 'session restored');
});

test('blob de sessao invalido responde bad_request e nao derruba o bridge', async () => {
  const h = harness();
  await h.bridge.handleCommand({ type: 'session_load', session: 'ISTO-NAO-E-JSON' });
  assert.equal(h.of('error').at(-1).code, 'bad_request');
  assert.deepEqual(h.exits, []);
});

test('modo invalido em start responde bad_request', async () => {
  const h = harness();
  await h.bridge.handleCommand({ type: 'session_load', session: null });
  await h.bridge.handleCommand({ type: 'start', mode: 'turbo' });
  assert.equal(h.of('error').at(-1).code, 'bad_request');
  assert.equal(h.sockets.length, 0);
});
