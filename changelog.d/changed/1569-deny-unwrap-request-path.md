- **Request-path do gateway nega `unwrap()`/`expect()` (#1569, primeira leva).** Dez
  modulos que recebem input externo (`rest_v1/{me,search,chats,groups,uploads,tasks/labels}`,
  `api`, `admin/store`, `diagnostics_handler`, `runs_handler`) ganham
  `#![deny(clippy::unwrap_used, clippy::expect_used)]`, e `clippy.toml` libera os dois
  em `#[cfg(test)]`. As 4 ocorrencias de producao viraram erro tipado (`400` em vez de
  panic na criacao de chat DM) ou constante checada em compile-time (iteracoes PBKDF2).
