- **Quality Ratchet: `runtime.rs` volta para baixo da baseline (#1254, plan
  0064).** O `mod tests` de `crates/garraia-agents/src/runtime.rs` (6 763 das
  10 881 linhas, o maior `.rs` do repositorio) vira `runtime/tests/`, um
  arquivo por tema com ate ~530 linhas — nao um `runtime_tests.rs` unico,
  porque o ratchet conta todo `.rs` rastreado e tambem quantos passam de
  700/1500/2500 linhas. Os submodulos aninhados viram arquivos irmaos sem
  mudar de caminho; helpers ficam `pub(super)` no tema que os criou. As tres
  guardas que varrem o fonte leem agora um `runtime.rs` que e so producao. O
  mesmo para `tools/repo_search_tool.rs` (840 → 433, testes em
  `repo_search_tool/tests.rs`) e para o smoke `tests/whatsapp_smoke.rs` da
  CLI (822 → 418 + `whatsapp_smoke_acesso.rs`), os dois que tinham cruzado as
  700 linhas desde a baseline. `max_file_lines` 10 881 → 6 901 e
  `files_over_700` 114 → 112; `files_over_1500`/`_2500` seguem um acima da
  baseline por `garraia-cli/src/whatsapp/acesso.rs` (1 650) e
  `garraia-cli/src/whatsapp/tests.rs` (3 726), registrados para decisao.
