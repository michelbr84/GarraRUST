// `node bridge.mjs --self-test` (#1636): importa as dependencias de execucao
// (Baileys, pino, sharp) e emite UMA linha JSON, sem abrir socket nem parear.
//
// Exige `npm ci` antes (como todo teste que toca Baileys de verdade): o CI
// instala as dependencias no job `whatsapp-bridge` antes do `node --test`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { SELF_TEST_DEPS } from '../bridge.mjs';
import { runBridge } from './helpers.mjs';

test('--self-test emite uma linha ok com a versao do Node e das dependencias', async () => {
  const { code, events } = await runBridge(['--self-test'], undefined, { timeoutMs: 60_000 });
  assert.equal(code, 0, 'autoteste tem de sair 0 com as dependencias instaladas');
  assert.equal(events.length, 1, 'o autoteste emite exatamente uma linha JSON');

  const [veredito] = events;
  assert.equal(veredito.self_test, 'ok');
  assert.equal(veredito.node, process.version, 'reporta a versao do Node que rodou');

  // Toda dependencia de execucao carregou e tem versao reportada.
  for (const name of SELF_TEST_DEPS) {
    assert.ok(name in veredito.deps, `faltou ${name} em deps`);
    assert.match(veredito.deps[name], /^\d+\.\d+\.\d+/, `${name} sem versao plausivel`);
  }
});
