- **Os testes do gateway nao leem nem gravam mais o `mcp.json`, o
  `allowlist.json`, o cofre e o `data/memory.db` de quem roda `cargo test`
  (#2027).** Uma auditoria com `HOME` falso e `strace` sobre os 187 binarios
  de teste achou tres caminhos que usavam o `<config_dir>` real
  (`~/.config/garraia`):
  - 14 testes de integracao montavam o `AppState::new`. Eles liam o
    `mcp.json` e o `allowlist.json` reais e, sem `mcp.json`, provisionavam um
    junto com o `data/workspace/`. O router de teste do `auth_test` fazia o
    mesmo quando nenhum outro teste do binario tinha chamado o `isolar()`
    antes dele;
  - os testes de unidade do gateway faziam o mesmo pelo `AppState::new`;
  - 27 testes de `bootstrap` chamavam o `build_agent_runtime` sem `data_dir`
    e abriam o `memory.db` real, com WAL e migrations.

  O `AppState::with_config_dir` passa a resolver as referencias `vault:` do
  `mcp.json` pelo cofre do config dir que recebe. Antes vinha sempre do cofre
  padrao, que decifrava os segredos reais quando a passphrase estava no
  ambiente. Ele tambem fica publico.

  Os testes de integracao montam o estado pelo
  `config_dir_descartavel::app_state`, sobre um diretorio descartavel. Nos
  testes de unidade, o `AppState::new` usa um config dir temporario por
  processo, e o `build_agent_runtime` recusa uma config sem `data_dir`. A
  guarda do #2021 passa a reprovar tambem quem chama o `AppState::new` sem
  apontar o `GARRAIA_CONFIG_DIR` no proprio arquivo. Producao nao muda: o
  `AppState::new` segue no config dir padrao, e o cofre resolvido e o mesmo
  de antes.
