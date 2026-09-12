- A varredura de segredos e o redactor de logs passam a cobrir o formato
  stateless dos installation tokens do GitHub (`ghs_` com pontos no corpo,
  ~520 chars; rollout 2026). `varredura-segredos.py` nao tinha padrao `ghs_`
  nenhum — token vazado passava batido; o redactor cobria `ghs_`, mas a classe
  de caracteres parava no primeiro ponto e vazava o restante para o log.
  Padroes na forma recomendada pelo GitHub (`ghs_` seguido de
  `[A-Za-z0-9.\-_]{36,}` no scanner), cobrindo stateful (40 chars) e
  stateless; tokens seguem tratados como opacos.
