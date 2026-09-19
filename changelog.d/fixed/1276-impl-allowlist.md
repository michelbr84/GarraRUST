- **A varredura de segredo do canal whatsapp-linked deixava todo `impl`
  gerado por `macro_rules!` passar.** A regra que proibe `SessionBlob` de
  ganhar conversao nova para `&str` olhava so para `impl` cuja linha contém
  "SessionBlob"; uma macro que gera `impl ::core::convert::AsRef<str> for $t`
  nao tem a palavra na linha do `impl` e chegava 111/111 verde. A regra agora
  e invertida (todo-impl): TODO `impl` de `session.rs` tem de estar na
  allowlist fechada `IMPL_ALLOWED` (9 entradas — as 4 originais de `SessionBlob`
  mais as 5 de `KeyOrigin`/`SessionKey`/`SessionStore`/`SessionError`),
  indentacao e cortada antes da comparacao, e o caso negativo entra como texto
  plantado — macro de ataque do issue, `impl Deref` manual a mao e `impl`
  indentado dentro de `mod` sao reportados; os 9 impls reais de producao
  confirmam que a allowlist continua viva.
