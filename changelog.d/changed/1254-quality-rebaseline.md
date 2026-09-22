- **O Quality Ratchet volta a comparar contra o estado real do codigo
  (#1254).** O baseline estava congelado em 2026-05-05 (maior arquivo com
  3240 linhas, 334 arquivos `.rs`) e todo PR recebia as mesmas quatro
  "regressoes", que eram deriva de meses e nao culpa de ninguem. O
  `freeze-baseline.py --adopt-current-file-metrics --reason '#1254'` foi
  rodado no SHA de `main` logo antes da tag e o `baseline.json` e o arquivo
  que ele gerou, sem edicao: so as metricas de tamanho de arquivo mudaram
  (hoje 608 arquivos, maior com 10513 linhas); auditoria, cobertura e clippy
  seguem a catraca estrita, e `audit.critical` continua 0. O teto de
  `thresholds.toml` (3500 linhas) nao subiu. Os 16 arquivos acima de 2500
  linhas ficam registrados como divida aceita no `.quality/README.md`, com o
  comando que reproduz o baseline.
