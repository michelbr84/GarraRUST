// Framing NDJSON: o que e erro de protocolo (codigo 3) e o que e so um comando
// invalido (evento `bad_request`, bridge continua vivo).
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  LineFramer,
  OversizeLineError,
  parseCommand,
  MAX_LINE_BYTES,
  EXIT_PROTOCOL,
  EXIT_OK,
} from '../bridge.mjs';
import { runBridge } from './helpers.mjs';

test('LineFramer divide por \\n e tolera \\r\\n', () => {
  const framer = new LineFramer();
  assert.deepEqual(framer.push(Buffer.from('a\nb\r\n')), ['a', 'b']);
  assert.deepEqual(framer.push(Buffer.from('par')), []);
  assert.deepEqual(framer.push(Buffer.from('tial\n')), ['partial']);
});

test('LineFramer rejeita linha completa acima do teto', () => {
  const framer = new LineFramer(16);
  assert.throws(() => framer.push(Buffer.from(`${'x'.repeat(20)}\n`)), OversizeLineError);
});

test('LineFramer rejeita linha NAO terminada acima do teto', () => {
  // E este o caso que o readline nao cobre: sem `\n` ele acumularia sem teto.
  const framer = new LineFramer(16);
  assert.throws(() => framer.push(Buffer.from('x'.repeat(20))), OversizeLineError);
});

test('parseCommand separa erro de framing de comando desconhecido', () => {
  assert.equal(parseCommand('{"type":"start"}').ok, true);
  assert.equal(parseCommand('{"type":"quemsabe"}').ok, true); // vira bad_request depois
  assert.equal(parseCommand('nao e json').ok, false);
  assert.equal(parseCommand('[1,2]').ok, false);
  assert.equal(parseCommand('{"sem":"type"}').ok, false);
  assert.equal(parseCommand('{"type":42}').ok, false);
});

test('--protocol-check imprime started valido e sai 0 sem tocar a rede', async () => {
  const { code, events, stderr } = await runBridge(['--protocol-check'], '');
  assert.equal(code, EXIT_OK);
  assert.equal(stderr, '');
  assert.equal(events.length, 1);
  assert.equal(events[0].type, 'started');
  assert.equal(events[0].protocol, 1);
  assert.equal(typeof events[0].bridge_version, 'string');
  assert.equal(typeof events[0].node_version, 'string');
});

test('linha ilegivel: error{code:protocol} e saida 3', async () => {
  const { code, events } = await runBridge([], 'isto nao e json\n');
  assert.equal(code, EXIT_PROTOCOL);
  const err = events.find((e) => e.type === 'error');
  assert.equal(err.code, 'protocol');
});

test('linha maior que 256 KiB: saida 3 mesmo sem \\n final', async () => {
  const { code, events } = await runBridge([], `{"type":"x","p":"${'a'.repeat(MAX_LINE_BYTES + 10)}"`);
  assert.equal(code, EXIT_PROTOCOL);
  assert.equal(events.at(-1).code, 'protocol');
});

test('comando desconhecido nao derruba o bridge', async () => {
  const stdin =
    '{"type":"session_load","session":null}\n{"type":"conversar"}\n{"type":"shutdown"}\n';
  const { code, events } = await runBridge([], stdin);
  assert.equal(code, EXIT_OK);
  const err = events.find((e) => e.type === 'error');
  assert.equal(err.code, 'bad_request');
  // ... e a sessao final sai mesmo sem nunca ter conectado.
  assert.ok(events.some((e) => e.type === 'session_update' && e.seq === 1));
});

test('start antes de session_load e bad_request', async () => {
  const { events } = await runBridge([], '{"type":"start","mode":"pair"}\n');
  const err = events.find((e) => e.type === 'error');
  assert.equal(err.code, 'bad_request');
  assert.match(err.message, /session_load/);
});
