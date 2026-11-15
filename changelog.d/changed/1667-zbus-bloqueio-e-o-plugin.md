- **Justificativa das excecoes de `zbus_names`/`zvariant` reavaliada (#1667).**
  O comentario do `deny.toml` e o espelho em `.cargo/audit.toml` afirmavam que o
  bloqueio era o `notify-rust` 4.18.2 "a mais nova, ainda em zbus 5". Isso
  deixou de ser verdade em 2026-11-08, quando o `notify-rust` 4.19.0 passou a
  declarar `zbus ^6`; o que mantem o `zbus` 5 no grafo e o proprio
  `tauri-plugin-notification`, que declara `zbus ^5` direto em todas as versoes
  publicadas (2.8.0, 2.8.1 e 3.0.0-alpha.3) — comprovado com
  `cargo tree -i zbus_names`. Nenhuma decisao de gate muda: os tres `RUSTSEC`
  seguem `unmaintained`, sem advisory de vulnerabilidade, com o prazo
  `EXPIRA 2026-12-31` intacto e verificado por
  `scripts/security/check_deny_exceptions_expiry.py`. O registro importa porque
  bumpar o `notify-rust` sozinho adicionaria um segundo `zbus` ao grafo sem
  remover as crates e pareceria (falsamente) que a excecao podia sair.
