- **Catalogo de funcionalidades acompanha o que entrou depois da 0.4.9.** O
  manifesto `crates/garraia-gateway/assets/catalogo-funcionalidades.json`
  (#1670), de onde o Garra tira a resposta sobre as proprias funcionalidades,
  ganha os itens `garra_diagnostics` (#1669) e `desktop` (app com o Assistente
  de Primeiro Uso, #1636), e corrige o `whatsapp_linked`: Node 20+ no host so
  e pre-requisito da CLI, porque o app desktop ja traz um Node privado;
  pareamento pelo assistente; cadencia entre turnos (#1650) e o alcance do
  `whatsapp self-test`. O caminho da allowlist no catalogo passa a ser o que
  se escreve no `config.yml` (`channels.whatsapp_linked.allow`, sem um
  `settings.` que o YAML nao tem, porque o campo e achatado); o mesmo vale
  para o `max_turnos_per_minute` da #1650. O `mcp-client` registra o jail fail-closed com o
  servidor filesystem fixado em 2.0.0 (#1684), e `instalacao-atualizacao`
  passa a citar o `install.ps1` e o canal de pre-lancamento. O runbook de
  release (`docs/releasing.md`) ganha o passo de subir o `catalog_version`
  junto com o bump do Cargo, que o teste da #1670 exige.
