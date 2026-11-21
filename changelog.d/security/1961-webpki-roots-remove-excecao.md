- **Sai a excecao do RUSTSEC-2026-0381 (`webpki-roots`), que nao suprimia mais
  nada (#1961).** O RustSec passou a listar o `webpki-roots` 0.26.11 como
  `unaffected` em 2026-10-21: ele e so um shim que reexporta as raizes da 1.x,
  e o lock ja resolve essa 1.x para a 1.0.10 (patched). A entrada some de
  `deny.toml` e do espelho em `.cargo/audit.toml` no mesmo commit, para que um
  eventual rebaixamento para versao afetada volte a ser apontado pelo
  `cargo-deny` e pelo `cargo audit`. Contribuicao de zyl71 (#1962).
