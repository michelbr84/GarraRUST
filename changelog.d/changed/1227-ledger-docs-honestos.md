- A rustdoc do `RunLedger`/`DbRunLedger` nao afirma mais que o gateway/CLI
  injetam o adapter: o wiring de producao (subida chamando
  `mark_interrupted_runs`, scheduler gravando run) e a #1227 e ainda nao
  existe. O fragmento da #1224 ja dizia a verdade; os doc comments agora
  dizem a mesma coisa.
