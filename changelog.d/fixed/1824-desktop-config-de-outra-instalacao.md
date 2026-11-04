- **O assistente do Desktop não grava mais na configuração de outra instalação
  sem perguntar (#1824).** Com um gateway GarraIA de fora do app na porta (um
  serviço do systemd, outra versão), os passos do assistente gravavam no
  `config.yml` dele: no teste da rc2, a persona trocou em silêncio o prompt de
  sistema de um bot do Telegram 0.5.1, e o modo "meu próprio número" podia abrir
  a admissão do WhatsApp dele. Agora a primeira tela pede uma confirmação
  explícita; provedor, persona e acesso recusam sem ela e, com ela, o app guarda
  uma cópia do `config.yml` em `backups/` antes de cada mudança. O pareamento do
  WhatsApp com um gateway de fora é recusado (abriria uma segunda conexão com a
  mesma conta), e o app não copia mais o config padrão para a pasta dele. Os
  Ajustes pedem a mesma confirmação.
