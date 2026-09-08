- memoria: com `shared_continuity` ligado, o recall do agente passa a usar a
  chave de continuidade **no lugar** do escopo de sessao, e nao somada a ele.
  O store faz AND entre os dois filtros, entao a flag nao compartilhava nada:
  uma sessao nova so enxergava o que ela mesma tinha gravado sob a mesma
  chave. Com a flag desligada nada muda — o escopo por sessao continua
  valendo. (#1042)
