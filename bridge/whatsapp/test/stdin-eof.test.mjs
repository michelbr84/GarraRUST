// EOF no stdin encerra a ponte. E a rede de seguranca cross-plataforma do
// #1636: quando o pai (o gateway/CLI) morre — inclusive de forma abrupta —, o
// SO fecha a ponta de escrita do pipe de stdin, a ponte ve EOF e sai. No macOS,
// onde nao ha PDEATHSIG, e o que impede o `node` de ficar orfao; no Windows e
// no Linux soma-se ao Job Object e ao PDEATHSIG. Nenhum socket de rede e aberto
// e nenhum destes testes precisa das dependencias do Baileys instaladas: a
// saida por EOF acontece antes de qualquer `session_load`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { runBridge } from './helpers.mjs';

test('EOF no stdin, sem nenhum comando, encerra com codigo 0', async () => {
  // `runBridge(args, stdin)` escreve `stdin` e fecha a ponta de escrita; `''`
  // e justamente "nada a dizer, pipe fechado" — o pai que morreu sem mandar
  // comando.
  const { code, events } = await runBridge([], '', { timeoutMs: 5000 });
  assert.equal(code, 0, `esperava saida 0 no EOF do stdin, veio ${code}`);
  assert.ok(
    events.some((e) => e.type === 'started'),
    'a ponte deve emitir `started` antes de encerrar',
  );
});

test('EOF no stdin encerra mesmo depois de processar uma linha', async () => {
  // Um `ping` antes do `session_load` so gera um erro de `bad_request` (nao e
  // fatal); o que importa e que, fechado o stdin logo em seguida, a ponte
  // encerra 0 em vez de ficar presa num `read` que nunca retorna.
  const { code } = await runBridge([], '{"type":"ping"}\n', { timeoutMs: 5000 });
  assert.equal(code, 0, `esperava saida 0 no EOF do stdin, veio ${code}`);
});
