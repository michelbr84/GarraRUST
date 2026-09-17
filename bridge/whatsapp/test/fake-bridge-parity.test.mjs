// A fixture Python existe para que os testes Rust rodem sem Node e sem
// telefone. Ela so serve para isso se falar EXATAMENTE o mesmo protocolo que a
// ponte real — e quem arbitra isso e o protocol-lint.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { lintStream } from '../scripts/protocol-lint.mjs';
import { BRIDGE_DIR } from './helpers.mjs';

const FIXTURE = path.join(
  path.dirname(BRIDGE_DIR),
  '..',
  'crates/garraia-channels/tests/fixtures/fake_whatsapp_bridge.py',
);
const hasPython = spawnSync('python3', ['--version']).status === 0;
const skip = !hasPython || !fs.existsSync(FIXTURE) ? 'python3 ou fixture ausente' : false;

function runFixture(args, stdin = '') {
  return spawnSync('python3', [FIXTURE, ...args], {
    input: stdin,
    encoding: 'utf8',
    timeout: 20_000,
  });
}

const FAST = ['--qr-expires', '0.05', '--handshake-timeout', '0.1'];

// cenario -> codigo de saida esperado (espelha os da ponte real)
const SCENARIOS = {
  'pair-ok': 0,
  'pair-expire-then-ok': 0,
  'logged-out': 2,
  'crash-after-qr': 1,
  'network-flap': 0,
  'serve-echo': 0,
};

for (const [scenario, expectedExit] of Object.entries(SCENARIOS)) {
  test(`fixture "${scenario}" passa no protocol-lint e sai ${expectedExit}`, { skip }, () => {
    const res = runFixture(['--scenario', scenario, ...FAST]);
    assert.equal(res.status, expectedExit, res.stderr);
    assert.deepEqual(lintStream(res.stdout).errors, []);
  });
}

test('fixture "connect-then-hang" conecta, entrega a sessao e emudece', { skip }, () => {
  const res = runFixture(['--scenario', 'connect-then-hang', ...FAST, '--hang-secs', '0.3']);
  assert.equal(res.status, 0);
  assert.deepEqual(lintStream(res.stdout).errors, []);
  const types = res.stdout.trim().split('\n').map((l) => JSON.parse(l).type);
  // O `session_update` e o ULTIMO evento: o silencio que o teto de flush
  // final do driver Rust cobre comeca exatamente aqui.
  assert.equal(types.at(-1), 'session_update');
  assert.ok(types.includes('connected'));
});

test('fixture "quiet-before-qr" emudece ANTES do QR, e nao depois', { skip }, () => {
  const res = runFixture(['--scenario', 'quiet-before-qr', ...FAST, '--hang-secs', '0.3']);
  assert.equal(res.status, 0);
  assert.deepEqual(lintStream(res.stdout).errors, []);
  const types = res.stdout.trim().split('\n').map((l) => JSON.parse(l).type);
  // O silencio cai entre o `status connecting` e o `qr`. E a forma medida do
  // Baileys real contra rede que nao alcanca o WhatsApp, e a unica janela em
  // que o driver Rust nao tinha nada a dizer ao usuario.
  assert.equal(types[0], 'started');
  assert.equal(types[1], 'status');
  assert.equal(types[2], 'qr');
  assert.ok(types.includes('connected'));
  assert.ok(types.includes('session_update'));
});

test('fixture "hang" fica muda depois do QR', { skip }, () => {
  const res = runFixture(['--scenario', 'hang', ...FAST, '--hang-secs', '0.3']);
  assert.equal(res.status, 0);
  assert.deepEqual(lintStream(res.stdout).errors, []);
  const types = res.stdout.trim().split('\n').map((l) => JSON.parse(l).type);
  assert.deepEqual(types, ['started', 'status', 'qr', 'status']);
});

test('fixture "pair-ok" emite o blob de sessao deterministico', { skip }, () => {
  const first = runFixture(['--scenario', 'pair-ok', ...FAST]).stdout;
  const second = runFixture(['--scenario', 'pair-ok', ...FAST]).stdout;
  assert.equal(first, second, 'a fixture tem de ser byte-a-byte deterministica');
  const update = first.trim().split('\n').map((l) => JSON.parse(l)).find((e) => e.type === 'session_update');
  assert.deepEqual(JSON.parse(Buffer.from(update.session, 'base64').toString('utf8')), {
    creds: { me: { id: '5511999990000:1@s.whatsapp.net' } },
    keys: {},
  });
});

test('fixture "serve-echo" responde send com sent + message de eco', { skip }, () => {
  const stdin = [
    '{"type":"session_load","session":null}',
    '{"type":"start","mode":"serve"}',
    '{"type":"send","request_id":"r1","chat_jid":"5511888880000@s.whatsapp.net","text":"oi"}',
    '{"type":"shutdown"}',
    '',
  ].join('\n');
  const res = runFixture(['--scenario', 'serve-echo', ...FAST], stdin);
  assert.equal(res.status, 0, res.stderr);
  assert.deepEqual(lintStream(res.stdout).errors, []);
  const events = res.stdout.trim().split('\n').map((l) => JSON.parse(l));
  assert.equal(events.find((e) => e.type === 'sent').request_id, 'r1');
  assert.equal(events.find((e) => e.type === 'message').text, 'echo: oi');
});

test('fixture recusa linha ilegivel com codigo 3, como a ponte real', { skip }, () => {
  const res = runFixture(['--scenario', 'serve-echo', ...FAST], 'isto nao e json\n');
  assert.equal(res.status, 3);
  const events = res.stdout.trim().split('\n').map((l) => JSON.parse(l));
  assert.equal(events.at(-1).code, 'protocol');
});
