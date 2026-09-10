- **#1105: `agent.bash_allowlist` deixa o operador declarar comandos
  confiaveis.** Um comando do tier arriscado do `safety_gate` morria
  fail-closed quando nao havia canal de confirmacao — era o caso de um CLI de
  outro agente instalado pelo proprio dono: bloqueado, sem como permitir. Os
  padroes da allowlist sao avaliados depois da denylist e antes do tier
  arriscado, e essa ordem e o ponto: a lista e positiva (so o que esta nela
  escapa da confirmacao) mas nao perdoa comando perigoso, que continua barrado
  mesmo que alguem o liste. Sintaxe pobre de proposito, para ser auditavel a
  olho nu: `prefixo*` com coringa so no fim, ou o comando exato. Padroes com
  coringa fora do fim sao recusados com warning em vez de interpretados; um
  `*` puro tambem e recusado — coringa sem prefixo casa com todo comando
  simples, e o tier arriscado desligado por um caractere nao e um padrao. E
  um prefixo nunca cobre comando composto (`;`, `&&`, pipe, `$(...)`,
  redirecao) — esse volta para o tier arriscado, que analisa cada segmento.
  Vale no gateway, no `garra chat` e no caminho MCP.
