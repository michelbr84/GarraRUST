- **Perfil de execucao `standard` | `isolated-pod` (ADR 0024, #1329) — secao
  `execution` e politica de boot.** Nova secao `execution` em `config.yml`
  (`profile: standard | isolated-pod`, `pod_root` opcional) e a env
  `GARRAIA_EXECUTION_PROFILE`, que vence o arquivo e e resolvida uma vez pelo
  `ConfigLoader` com a origem registrada (`default` | `file` | `env`). Secao
  ausente = `standard` = comportamento de hoje. Valor invalido (arquivo ou
  env) e erro de carga: o gateway nao sobe, e `garra config check` reporta
  `Error` em `execution.profile` (exit 2) em vez de cair em `standard` em
  silencio. A env nunca e promovida ao arquivo por um `save`. O `config check`
  mostra perfil e origem no sumario e avisa quando `execution.pod_root` esta
  em `standard` ou e relativo, e quando `channels.whatsapp_linked.owners`
  esta preenchido fora de `isolated-pod` (so a contagem, nunca as
  identidades). O gateway ganha `bootstrap::execution` — politica pura com a
  raiz do MCP `filesystem` por perfil (`agent.file_roots` ou
  `<data_dir>/workspace` em `standard`; `execution.pod_root` ou o mesmo
  workspace em `isolated-pod`; **nunca** `$HOME`) e o anuncio de boot
  (`info!` em `standard`; `warn!` unico em `isolated-pod` dizendo o que foi
  liberado, o que o perfil NAO isola e como reverter). Nenhum codigo le
  marcador de container para decidir o perfil; dois testes varrem o fonte e
  proibem esses literais.
