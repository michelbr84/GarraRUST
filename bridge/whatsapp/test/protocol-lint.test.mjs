// O lint e o arbitro entre a ponte real e a fixture Python: se os dois passam
// nele, os dois falam o mesmo protocolo.
import test from 'node:test';
import assert from 'node:assert/strict';
import { lintStream } from '../scripts/protocol-lint.mjs';

const STARTED =
  '{"type":"started","protocol":1,"bridge_version":"1.0.0","baileys_version":"7.0.0-rc14","node_version":"22.0.0"}';

test('fluxo minimo valido', () => {
  assert.deepEqual(lintStream(`${STARTED}\n`).errors, []);
});

test('baileys_version null e valido (dependencias nao instaladas)', () => {
  const line = '{"type":"started","protocol":1,"bridge_version":"1.0.0","baileys_version":null,"node_version":"22.0.0"}';
  assert.deepEqual(lintStream(`${line}\n`).errors, []);
});

test('stream vazio ou sem started e recusado', () => {
  assert.match(lintStream('').errors[0], /empty stream/);
  assert.match(lintStream('{"type":"log","level":"info","message":"oi"}\n').errors[0], /must be "started"/);
});

test('campo faltando ou enum invalido e apontado', () => {
  const bad = `${STARTED}\n{"type":"status","state":"voando"}\n`;
  assert.match(lintStream(bad).errors[0], /field "state" invalid/);
  const missing = `${STARTED}\n{"type":"qr","data":"x"}\n`;
  assert.match(lintStream(missing).errors[0], /field "expires_in_secs" invalid/);
});

test('campo a mais e erro: o Rust desserializa com schema fechado', () => {
  const extra = `${STARTED}\n{"type":"authenticated","extra":1}\n`;
  assert.match(lintStream(extra).errors[0], /unexpected field "extra"/);
});

test('session_update precisa de seq crescente e base64 de JSON', () => {
  const blob = Buffer.from('{"creds":{},"keys":{}}').toString('base64');
  const ok = `${STARTED}\n{"type":"session_update","session":"${blob}","seq":1}\n{"type":"session_update","session":"${blob}","seq":2}\n`;
  assert.deepEqual(lintStream(ok).errors, []);
  const repeated = `${STARTED}\n{"type":"session_update","session":"${blob}","seq":1}\n{"type":"session_update","session":"${blob}","seq":1}\n`;
  assert.match(lintStream(repeated).errors[0], /seq must increase/);
  const garbage = `${STARTED}\n{"type":"session_update","session":"nao-e-json","seq":1}\n`;
  assert.match(lintStream(garbage).errors[0], /not base64\(JSON\)/);
});

test('tipo de evento desconhecido e recusado', () => {
  assert.match(lintStream(`${STARTED}\n{"type":"inventado"}\n`).errors[0], /unknown event type/);
});
