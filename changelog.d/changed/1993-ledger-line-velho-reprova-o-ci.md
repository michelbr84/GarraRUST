- **`line` velho no ledger CodeQL reprova o CI (#1993, item 2).**
  `check-ledger-anchors.py` ganhou `--fail-on-stale-line`: quando a posicao
  derivada do `sink_snippet` diverge do `line` commitado (no `.json` ou na
  coluna `File:line` do `.md`), o checker sai com exit 8 em vez de so avisar.
  O job `CodeQL Ledger Anchors` passa a usar a flag. Antes, o `line` ficava
  para tras no `main` em silencio e o `codeql-reapply-dismissals.sh` abortava
  (`exit 2`) na primeira entrada divergente, sem conferir as seguintes. O
  conserto continua mecanico: rodar o checker sem flags e commitar o `line`
  reescrito. Drift de statement (exit 6) e precondicao (exit 5) seguem
  vencendo, porque pedem reauditoria humana.
