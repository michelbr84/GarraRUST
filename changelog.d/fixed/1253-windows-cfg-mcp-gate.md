- O codigo `#[cfg(windows)]` sob a feature `mcp` (wrapper `cmd /c` do
  `manager.rs`) agora compila no CI: um step Windows-only no job matrix roda
  `cargo check -p garraia-agents --features mcp --all-targets`, onde a
  toolchain MSVC nativa do runner ja existe. O caminho mingw cross foi
  descartado com medicao (#1253).
- A doc agora diz explicitamente que o hardening de permissoes de arquivo
  e so Unix: `config.yml` no Windows herda a ACL default do diretorio, e os
  modos `0600`/`0700` da sessao WhatsApp nao se aplicam la (#1253).
