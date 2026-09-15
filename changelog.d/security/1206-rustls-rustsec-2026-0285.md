- **`rustls` atualizado para 0.23.45, corrigindo RUSTSEC-2026-0285 (#1206).** O
  advisory (TLS 1.3 handshake messages aceitas incorretamente entre fronteiras
  de nivel de criptografia, severidade 5.3/medium) foi publicado em 2026-09-14
  contra a versao `0.23.40` ja fixada no `Cargo.lock`. Bump transitivo via
  `cargo update -p rustls --precise 0.23.45` (dependencia de `reqwest`/
  `rustls-tls` usada pelo gateway e pelas crates que fazem chamadas HTTP de
  saida); `aws-lc-rs`/`aws-lc-sys`/`rustls-webpki` acompanharam a atualizacao.
