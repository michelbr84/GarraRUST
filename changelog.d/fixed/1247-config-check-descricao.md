- **Prosas do repo deixam de afirmar um gate de boot que nao existe.** O
  `garra config check` so e invocado por dois comandos opt-in
  (`garra config check` e `garra doctor`) — nada no caminho de boot do
  gateway roda o `run_check`, entao um `Severity::Error` na config nao
  impede o gateway de subir. Duas prosas afirmavam o contrario: o
  doc-comment do `spawn_hardware_adapters` ("mostra os mesmos erros de
  config antes do boot") e o comentario do teto de risco das automacoes
  ("o `garra config check` ja recusa R3+ antes do boot") foram reescritos
  para descrever o comando pelo que ele e (report opt-in) e a camada viva
  de verdade (o teto invalido cai no default r1 no boot). A entrada de
  `allowed_origins: ["*"]` no threat model tambem cita o check como
  "Validacao" — virou "Report (opt-in)". Varredura do repo por afirmacoes
  de gate nao achou mais nenhum ponto falso; os demais usos do comando na
  doc ja o descrevem como validacao de leitura.
