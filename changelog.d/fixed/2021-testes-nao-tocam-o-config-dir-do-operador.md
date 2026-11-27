- **Os testes do gateway nao tocam mais o `master.key` nem o `admin.db` de quem
  roda `cargo test` (#2021).** Dois caminhos usavam o `<config_dir>` real
  (`~/.config/garraia`):
  - o router de teste (`test-helpers`) resolvia a chave do admin contra o
    `<config_dir>/admin/master.key` real com um store vazio. Um `master.key`
    de tamanho errado era movido para `master.key.unreadable` e trocado por
    outro, e o gateway de verdade, com segredos no `admin.db`, caia em 503;
  - `auth_test`, `gateway_integration`, `voice_e2e_test` e `bind_fail_closed`
    subiam o `GatewayServer` sem `GARRAIA_CONFIG_DIR`. So os dois testes de
    chave de API do `auth_test`, que rodam em todo `cargo test`, ja abriam o
    `admin.db` real e criavam `master.key`, `sessions.db` e `mcp.json` no
    config dir do operador.

  `resolve_admin_encryption_key` passa a receber o diretorio do admin: o
  boot de producao passa `default_admin_dir()`, e o router de teste um
  diretorio descartavel. Os quatro testes desviam o config dir para um
  temporario, e um teste novo varre os `tests/` e reprova quem subir o
  `GatewayServer` sem isso.
