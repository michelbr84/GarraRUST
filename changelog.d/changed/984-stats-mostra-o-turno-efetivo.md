- **`/stats` passa a dizer o que a ultima resposta usou de verdade (#984).**
  Mostrava tres contadores globais — sessoes ativas, overrides de modelo, tarefas
  A2A — e nada sobre o LLM. Agora mostra provider, modelo, ferramentas
  executadas, tokens, latencia e se um fallback respondeu.
- **Efetivo, e nao configurado.** A issue faz a distincao certa: o runtime
  resolve override, prefixo de modelo, `tools_model` e fallback pelo caminho,
  entao perguntar a config e perguntar a quem nao sabe. O runtime passa a
  registrar, no fim de cada turno, o que de fato aconteceu — e o modelo vem da
  **resposta do provider**, o unico valor que sobreviveu a todas as resolucoes.
- **O que nao da para saber aparece como nao sabido.** No streaming nao ha
  `LlmResponse`: so deltas de texto. Ali o modelo conhecido e o *pedido* e nao ha
  contagem de tokens, entao o `/stats` marca os dois em vez de mostrar o pedido
  como se fosse o efetivo. Zero-porque-nao-sei e diferente de
  zero-porque-nao-usou.
- **Modo aplicado, e nao apenas o salvo.** O #988 separou escolha de deducao, e
  so a escolha liga a `ToolPolicy`. O `/stats` diz qual dos dois esta em vigor —
  mostrar so o modo salvo faria o usuario acreditar numa restricao que nao vale.
  Mostra o objetivo da sessao (#983) junto.
- O registro por sessao tem teto de 512 entradas com despejo do mais antigo: a
  chave e o `session_id`, que vem de request, e sem teto o mapa cresceria com o
  numero de sessoes que ja passaram. Batimento agendado (`process_heartbeat`)
  **nao** grava — sobrescrever o `/stats` com um turno que o usuario nao pediu
  seria pior que nao ter o dado.
- Os dois `unwrap()` do closure do `/stats` sairam (regra absoluta 4). Os outros
  24 de `commands.rs` continuam la, fora do escopo deste trabalho.
