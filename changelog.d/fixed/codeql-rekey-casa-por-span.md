- **`codeql-rekey-ledger.py` casa a linha do sink dentro do span do alerta.** O
  CodeQL reporta o span do statement (`start_line`..`end_line`) e o ledger
  ancora a linha do sink, que num `println!` multi-linha e a ultima do span; o
  rekey comparava `start_line` exato e deixava a duplicata aberta como "sem
  entrada" — foi o que manteve o alerta #176 (mesmo sink do #173 dispensado,
  `garra whatsapp link`) aberto desde a v0.4.5. Agora a regra e a mesma do
  `codeql-reapply-dismissals.sh`, com testes, e o ledger reaponta #173 → #176.
