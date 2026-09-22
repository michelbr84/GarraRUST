- **`freeze-baseline.py` ganha um re-baseline auditado:
  `--adopt-current-file-metrics --reason '#NNN'` (#1254).** Ate aqui a
  ferramenta so sabia apertar o ratchet: com o baseline de 2026-05-05 muito
  atras do main, qualquer proposta saia igual ao baseline velho, e a unica
  saida seria editar `.quality/baseline.json` a mao, o que e proibido. A flag
  adota do `current-metrics.json` apenas as metricas de tamanho de arquivo;
  audit, cobertura e clippy continuam no ratchet estrito e `audit.critical`
  segue 0. O `--reason` tem de citar uma issue, o arquivo gerado registra
  `adopted_reason`, `source_git_sha` e `source_collected_at` para qualquer um
  reproduzir, e a flag recusa `--seed` e um `--out` que aponte para o baseline:
  continua escrevendo so o `baseline.proposed.json`. Sem a flag, nada muda.
