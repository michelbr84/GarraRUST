- **Retencao do ledger de runs com `runs.retention_days` (#1227).** A tabela
  `agent_runs` ganhava uma linha por run agendado e nada a encurtava. A nova
  secao de topo `runs.retention_days` liga uma varredura no gateway (no boot e
  a cada 24 horas) que apaga runs terminais mais velhos que a janela. O
  default e `0` = nunca apaga, entao uma atualizacao nao remove historico;
  desligada, o gateway avisa uma vez no boot quantos runs existem. Run
  `running` nunca e apagado, qualquer que seja a idade, e instante ilegivel
  tambem fica. A chave e de topo porque `agents` e um mapa de agentes
  nomeados. `garraia config check` recusa valor acima de 3650, e o log da
  varredura leva so contagem, nunca conteudo do run.
