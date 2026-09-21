- **Sandbox: `backend: ssh` passa a recusar fail-closed a policy que nao consegue
  honrar (#1225 S3, ADR 0019).** O ramo SSH do `wrap_command` monta
  `ssh <host> -- sh -lc ...` e nada mais — nao existe `--network none` nem mount do
  `cwd` numa sessao ssh —, e ate aqui ele simplesmente **ignorava**
  `agent.sandbox.network_disabled` e `agent.sandbox.mount_workdir`. Como o default das
  duas e `true`, um `agent.sandbox` com `backend: ssh` e `ssh_host` e mais nada lia como
  "rede desligada, workdir contido" quando nenhuma das duas era verdade: fail-open por
  omissao, na chave que promete contencao. O `garra config check` so avisava.
  Agora a recusa mora no proprio `wrap_command` — o ponto unico por onde gateway,
  `garra chat` e `garra mcp-agent` passam, e que tambem cobre policies montadas por
  codigo sem passar por config nenhuma: enquanto qualquer uma das duas flags estiver
  `true`, a tool `bash` responde erro claro nomeando a chave real e a acao, e nenhuma
  linha `ssh ...` e montada. A checagem roda antes do `is_available()`, para que num host
  sem cliente ssh o erro de "backend nao encontrado" nao mascare este. O
  `sandbox_policy_from` NAO desliga as flags nem rebaixa o modo por conta propria — passa
  tudo intacto e emite `warn!` no boot (sem o host) para o problema ter nome antes do
  primeiro comando recusado. O `garra config check` sobe de Warning para **Error**, um por
  chave ligada, com a mesma mensagem.
  A unica forma de `ssh` passar e `agent.sandbox.network_disabled = false` **e**
  `agent.sandbox.mount_workdir = false` explicitos: o `false` e o reconhecimento do
  operador de que ssh e execucao remota SEM isolamento de rede nem de mount. `docker` e
  `podman` honram as duas e nao sao afetados. A superficie `agent.sandbox` entrou depois
  da v0.4.2 e nao saiu em release, entao nao ha migracao para ninguem. Tabela de garantias
  por backend em `docs/security/threat-model.md` secao 5.13 atualizada, com o exemplo minimo de
  config ssh que passa.
