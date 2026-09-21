- **Perfil de execucao e raiz do MCP `filesystem` visiveis no console (ADR
  0024, #1329).** `GET /api/diagnostics` ganha duas linhas: `execution.profile`
  (`ok` em `standard` com a fonte; `warning` permanente em `isolated-pod`
  dizendo a fonte, o piso do WhatsApp pessoal, a CONTAGEM de `owners` — nunca
  as identidades — e a raiz do MCP, com o passo de reversao pelos dois
  caminhos) e `mcp.filesystem_root` (`skipped` sem servidor `filesystem`;
  em `standard`, `warning` nomeando a primeira raiz persistida fora de
  `agent.file_roots` / `<data_dir>/workspace`, comparada canonicamente
  quando os diretorios existem; `ok` em `isolated-pod`, onde o pod e a
  fronteira). `GET /api/settings/{schema,effective}` ganha as linhas
  read-only `security.execution_profile` (`standard` | `isolated-pod`, com a
  origem real `default` | `file` | `env`) e `security.execution_pod_root`.
