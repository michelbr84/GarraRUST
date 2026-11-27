- **Os testes do gateway nao usam mais o `mcp.json`, o `allowlist.json`, o
  cofre nem o `memory.db` de quem roda `cargo test` (#2027).** Continuacao do
  #2021:
  - 14 testes de integracao montavam o `AppState::new` sem desviar o config
    dir. Criavam `mcp.json` e `data/workspace/` em `~/.config/garraia`, liam a
    allowlist real do operador e ligavam o cofre do caminho padrao, que
    decifraria as referencias `vault:` do `mcp.json` real se a passphrase
    estivesse no ambiente. Agora chamam `config_dir_descartavel::isolar()`, e
    a varredura do #2021 tambem reprova quem chamar `AppState::new` sem isso;
  - os testes de unidade de `bootstrap` chamavam `build_agent_runtime` com
    `data_dir: None`, que abria (e migrava) o `<config_dir>/data/memory.db`
    real. Sem `data_dir` proprio, cada chamada ganha um tempdir.
