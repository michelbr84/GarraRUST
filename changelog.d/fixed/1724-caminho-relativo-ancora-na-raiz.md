- **Caminho relativo nas file tools resolve na raiz declarada, nao no CWD do
  processo (#1724).** No gateway com `agent.file_roots` e sessao sem
  `working_dir`, `notas.txt` era recusado como "fora das raizes" porque o jail
  ancorava o relativo no CWD do processo, que nao e raiz; o modelo repetia ate
  derrubar o breaker e a tarefa falhava ou era adiada por heartbeat. Agora
  `FileJail::confine` ancora o relativo, nesta ordem, no `working_dir` da sessao,
  no CWD se ele proprio estiver dentro de uma raiz (a CLI) ou na primeira raiz
  configurada -- e o resultado continua passando pela checagem de raiz, entao o
  jail nao se amplia (`..` e caminho fora seguem recusados). O confinamento do
  MCP `filesystem` segue a mesma regra.
