- **`cli_inspect` passa a mostrar os efeitos de ferramenta que o executor
  registrou (#1709).** O agente conseguia o estado da sessão (versão, provider,
  gateway declarado, ferramentas liberadas), mas não a trilha do que de fato
  rodou — e uma pergunta como "você escreveu o arquivo?" só podia ser respondida
  de memória do modelo. A tool agora projeta a trilha que o `ToolTrail` fecha a
  cada turno em classes de evidência (`observado`, `falhou`, `nao_verificado`)
  com o id da chamada, usando a camada `garraia_agents::evidence` (#1709): só
  `Ok` do executor vira `observado` com referência; falha/bloqueio vira
  `falhou`; chamada interrompida ou em execução vira `nao_verificado` — ausência
  de resultado não é prova de que não aconteceu. A saída é limitada às últimas
  20 execuções com o total ao lado, não carrega `input`, `output`, `detail` nem
  caminho, e sem turno com ferramenta a seção responde `unverified` com motivo
  em vez de uma lista vazia que sugeriria "nada rodou".
