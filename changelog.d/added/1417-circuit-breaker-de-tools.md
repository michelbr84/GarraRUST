- **Circuit breaker para ferramentas que falham repetidamente (#1417).** No
  dogfood da v0.4.5 o modelo repetia `repo_search` varias vezes no mesmo
  turno depois de uma falha que nao ia mudar (sem repositorio) e de timeouts.
  O runtime ganha um breaker por sessao e por ferramenta
  (`garraia_agents::tools::breaker`, estado puro, relogio injetado), no unico
  ponto de despacho: falha deterministica (sem raiz para as file tools,
  `repo_search` sem repositorio) poe a ferramenta em pausa ate o fim do
  turno; timeout abre um cooldown que dobra a cada timeout seguido (15s ate
  120s) e atravessa turnos; erro generico so abre depois de tres iguais no
  turno — e a recusa de caminho fora das raizes e generica de proposito,
  porque vale para um caminho e o modelo corrige o caminho na chamada
  seguinte. Em pausa, a chamada nao roda e o modelo recebe o motivo
  estruturado ("temporariamente indisponivel nesta conversa (`no_roots` |
  `no_repository` | `timeout` | `repeated_error`)") com a instrucao de nao
  repetir. Sucesso fecha o breaker
  da ferramenta; `working_dir` diferente num turno novo limpa a sessao.
  `garra_status` devolve `breaker` (tool, `reason_code`, `reason`) e
  `breaker_means`; a nota do prompt manda o modelo respeitar a lista. O
  agregado no `/api/diagnostics` fica para a #1438.
