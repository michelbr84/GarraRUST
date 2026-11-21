- **Um predicado so decide se a chamada tem resultado observado (#1915).**
  `DesfechoDaChamada::resultado_observado()` (match exaustivo; hoje so
  `Cancelada` fica de fora) passa a ser a regra unica que o ledger de
  evidencia (`evidence_from_event`) e o historico de ferramentas da CLI
  consultam, em vez de cada um casar a variante `Cancelada`. Um desfecho
  "nao observado" futuro nao volta a virar falha registrada sem alguem
  decidir de que lado ele fica. O `success` da evidencia passa a sair do
  `outcome`, a mesma fonte que decide se ha evidencia. Comportamento
  inalterado; nenhum modo, teto, raiz ou jail mudou.
