- **`.quality/baseline.json` deixa de dizer `coverage_pct: null` (#1566).** O
  #1573 consertou a corrida que impedia o ratchet de achar o `lcov.info` e
  fixou os numeros num doc versionado, mas o baseline estruturado — o arquivo
  que o `compare.py` de fato le — continuava em
  `{"coverage_pct": null, "status": "not_collected_yet"}`, congelado em
  2026-09-22 (`af3227c`). Agora traz 72,00% de linha (110.863 / 153.982, 451
  arquivos), medidos pelo job `Coverage (cargo-llvm-cov)` do run 37538157886
  em `main` no commit `13e6f1dc`, e gravados pelo `freeze-baseline.py` a
  partir do `lcov.info` daquele run — nao a mao, o invariante anti-fraude
  continua valendo. No mesmo passo o ratchet estrito **aperta** o
  `max_file_lines` de 10.513 para 6.933, o valor real, e corrige o
  `max_file_path` para `crates/garraia-config/src/check.rs`: o antigo apontava
  para `garraia-agents/src/runtime.rs`, que ja nao e o maior arquivo do repo.
  Os contadores `files_over_{700,1500,2500}` ficam nos valores mais apertados
  (112 / 40 / 16) em vez de serem relaxados para a medicao de hoje
  (123 / 45 / 18) — o relatorio passa a registrar essas tres como regressao
  real, o que elas sao. Segue `report-only`: nenhuma PR e bloqueada.
