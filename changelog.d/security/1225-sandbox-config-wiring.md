- **O sandbox por tool passa a ser alcancavel pelo operador — `agent.sandbox`
  (#1225).** A contencao entregue na #1222 (e corrigida na #1231) existia
  inteira: `SandboxPolicy`, `SandboxMode`, `SandboxBackend`, `wrap_command`
  com quoting POSIX, fail-closed quando o backend falta, e onze testes
  unitarios verdes. E nenhuma instalacao conseguia liga-la. Os tres
  construtores de producao do `BashTool` fixavam `SandboxPolicy::default()`
  (= `off`), `set_sandbox_policy` so era chamado pelos proprios testes do
  modulo, e a chave de config que as mensagens de erro citavam
  (`tools.sandbox.backend`, `tools.sandbox.mode`) nao existia em lugar nenhum
  do schema — `grep -ri sandbox crates/garraia-config/src/` nao devolvia nada.
  Uma funcionalidade de seguranca que so os testes dela conseguem exercitar
  nao e uma funcionalidade; e uma que parece existir no changelog.
  Agora ha a secao `agent.sandbox` (`mode`, `backend`, `image`, `ssh_host`,
  `sandboxed_tools`, `elevated`, `mount_workdir`, `network_disabled`), lida
  nos tres pontos de producao pela MESMA funcao
  (`garraia_gateway::bootstrap::sandbox_policy_from`), como ja acontece com
  os adapters de hardware: gateway, `garra chat` e `garra mcp-agent` nao podem
  discordar sobre onde um comando roda. O caminho MCP e o que mais ganha —
  la nao existe canal de confirmacao humana, entao o sandbox e a unica camada
  que pode conter o que passa do tier arriscado.
  Secao ausente continua sendo `mode: off`: zero mudanca de comportamento
  para quem ja tem config, e ha teste afirmando que o default da secao e
  byte a byte o `SandboxPolicy::default()`.
- **`garra config check` passa a recusar sandbox que parece ligado e nao
  esta (#1225).** Erro quando `mode != off` sem `backend` (todo comando falha
  fechado, o que e seguro e inutil) e quando `backend: ssh` esta sem
  `ssh_host`. Aviso quando `backend: ssh` vem com `network_disabled` ou
  `mount_workdir` ligados, dizendo com todas as letras que o backend SSH e
  execucao remota e nao sandbox, e que ele ignora os dois campos; quando
  `elevated` (as tools que escapam e rodam NO HOST) esta preenchido sem
  `tool_confirmation_enabled`; e quando `mode: allowlist` vem com
  `sandboxed_tools` vazio, que sandboxa exatamente nada. Nenhum finding ecoa
  o `ssh_host`, e ha teste so para isso.
- **Documentado o que cada backend realmente garante (#1225).**
  `docs/security/threat-model.md` ganhou a secao 5.12 com a tabela backend x
  garantia (rede, sistema de arquivos, privilegios, onde roda, o que NAO
  cobre) e tres limites que estavam so no codigo: hoje somente a tool `bash` e
  envolvida — `run_tests`, `git_diff`, `code_review` e `repo_search` seguem
  nascendo no host mesmo com `mode: all`; o backend `ssh` isola o host local e
  nada mais; e na pratica isto e unix, porque no Windows o bash tool usa
  `powershell -Command` e nao reparseia o quoting POSIX do wrap. O
  `config.hardened.example.yml` traz o exemplo com os mesmos avisos, e
  `/api/settings` mostra `security.sandbox_mode` e `security.sandbox_backend`
  como leitura (o PATCH da rota ainda e dry-run, entao um controle de
  contencao nao pode aparecer como editavel la). As mensagens de erro do
  `wrap_command` foram corrigidas para a chave que passa a existir.
