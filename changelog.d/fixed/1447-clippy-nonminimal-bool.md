- **`cargo clippy -D warnings` limpo no stable 1.95 (#1447).** Duas
  expressoes booleanas pre-existentes disparavam `clippy::nonminimal_bool`
  a partir do rustc 1.95: o predicado de `retain` em
  `whatsapp_linked.rs` (De Morgan) e uma asserção de teste em
  `safety_gate.rs` (`!x.is_ok()` → `x.is_err()`). Sem mudanca de
  comportamento; nenhum `#[allow(...)]` novo.
