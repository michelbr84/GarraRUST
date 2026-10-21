- **Diagnostico nativo e seguro da instalacao do Garra (#1669).** Nova tool de
  agente `garra_diagnostics` no `garraia-gateway`: relatorio estruturado e
  somente leitura da propria instalacao — versao instalada, estado do gateway,
  todos os canais com estado, servidores MCP, capacidades e ferramentas
  disponiveis, configuracao efetiva validada (`garraia config check`),
  presenca e ORIGEM de credenciais sem nunca revelar valores (vault /
  config.yml / env), amostra limitada e redigida das ultimas linhas do log e
  componentes relevantes (ponte Node do WhatsApp, banco alcancavel). Saida em
  JSON com uma entrada por verificacao `{id, estado, evidencia, detalhe}` e
  vocabulario fechada de estados (`ok`, `erro`, `nao_configurado`,
  `bloqueado`, `degradado`, `nao_suportado`). Controle de acesso por sessao do
  operador + turno (`sessao_do_operador`/`turno_restrito`), fail-closed: quem
  nao e o operador recebe uma recusa e nada mais. File jail para a leitura do
  log (so dentro do diretorio do produto), redacao de segredos
  (`garraia_security`) em todo texto e zero shell. Refs #1669
