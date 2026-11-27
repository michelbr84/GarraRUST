- **Ledger CodeQL: entrada cujo alerta o `main` nao detecta mais aparece na
  triagem (#1993, itens 1 e 3).** `codeql-rekey-ledger.py` ganhou
  `--check-main`: consulta a instancia de cada entrada em `refs/heads/main`
  (`/code-scanning/alerts/{n}/instances`) e sai com exit 3 listando as que
  ficaram `fixed`, sem instancia ou inexistentes. Um alerta dispensado nunca
  passa a `fixed`, entao antes nenhum script percebia; em 2026-11-25 eram 14
  de 43 entradas. O `codeql-triage.yml` usa a flag e so devolve o exit depois
  de publicar o job summary (antes, o exit 2 de drift matava o step sem
  publicar nada). Os testes do rekey passam a rodar no job `CodeQL Ledger
  Anchors` do CI, sem pytest. A conferencia do conteudo das citacoes
  `arquivo:linha` virou passo manual explicito da re-auditoria de 90 dias
  (§3 regra 4), sem gate.
