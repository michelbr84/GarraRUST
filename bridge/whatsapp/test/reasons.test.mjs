// Mapeamento de queda e agenda de backoff — o Rust depende destes valores.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mapDisconnectReason, backoffDelayMs, EXIT_LOGGED_OUT, EXIT_OK } from '../bridge.mjs';

test('515 reconecta na hora (pos-pareamento do WhatsApp)', () => {
  assert.deepEqual(mapDisconnectReason(515), { reason: 'restart_required', retry: 'immediate' });
});

test('codigos nao-autorizados do Baileys viram logged_out com saida 2', () => {
  for (const code of [401, 403, 419]) {
    assert.deepEqual(mapDisconnectReason(code), {
      reason: 'logged_out',
      retry: 'never',
      exit: EXIT_LOGGED_OUT,
    });
  }
});

test('440 e substituicao: nao reconecta, mas a sessao segue valida', () => {
  assert.deepEqual(mapDisconnectReason(440), { reason: 'replaced', retry: 'never', exit: EXIT_OK });
});

test('timeout, rede e desconhecido entram no backoff', () => {
  assert.equal(mapDisconnectReason(408).reason, 'timeout');
  for (const code of [411, 428, 500, 503]) {
    assert.equal(mapDisconnectReason(code).reason, 'network');
  }
  assert.deepEqual(mapDisconnectReason(0), { reason: 'unknown', retry: 'backoff' });
  assert.deepEqual(mapDisconnectReason(9999), { reason: 'unknown', retry: 'backoff' });
});

test('backoff sem jitter: 1s -> 30s, dobrando, com teto duro', () => {
  const zero = () => 0;
  const schedule = [1, 2, 3, 4, 5, 6, 7, 8].map((n) => backoffDelayMs(n, zero));
  assert.deepEqual(schedule, [1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]);
});

test('jitter adiciona ate 25% e nunca ultrapassa 30s', () => {
  const almostOne = () => 0.999999;
  assert.equal(backoffDelayMs(1, almostOne), 1249);
  assert.equal(backoffDelayMs(6, almostOne), 30000);
  for (let attempt = 1; attempt <= 12; attempt += 1) {
    const value = backoffDelayMs(attempt, Math.random);
    assert.ok(value >= 1000 && value <= 30000, `attempt ${attempt} -> ${value}`);
  }
});
