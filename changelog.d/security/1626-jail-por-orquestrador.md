- **Jail e workdir por orquestrador no `garra_agent` do MCP HTTP (#1626, item 1).**
  Cada orquestrador da ponte HTTP passa a ter uma subarvore gravavel propria sob
  `<data_dir>/workspace`, derivada da identidade dele por digest (SHA-256, 32 hex),
  que vira a unica raiz efetiva das file tools (jail sem raiz fixa) e o workdir do
  sandbox do `bash`. Antes o jail e o workdir eram globais (`agent.file_roots`),
  iguais para todo orquestrador: o que um escrevia o outro lia — disclosure e
  prompt-injection indireta cross-principal, a mesma classe que a #1449 fechou para
  os canais. `agent.file_roots` deixa de alcancar o agente da ponte (ela atende
  pedido de terceiro); sem workspace materializado o agente cai em fail-closed, com
  as file tools negadas e o `bash` recusado. As mensagens das file tools e do
  `git_diff`/`code_review` que iam ao orquestrador passam a mostrar o caminho
  RELATIVO ao workspace, nunca o caminho absoluto do host (que traria `<data_dir>`
  e o hash do proprio chamador) — principio da #1465. Limite conhecido: em
  `isolated-pod` todos os orquestradores compartilham o filesystem do pod, entao o
  workdir isola o `bash` so no sandbox docker/podman.
