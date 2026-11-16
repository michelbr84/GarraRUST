- **Aprovacao, ferramenta e pausa passam a ler o mesmo workspace do turno (#1912).**
  O runtime resolvia o diretorio efetivo tres vezes por turno (na aprovacao, em cada
  `ToolContext` e no registro da pausa); se o disco mudasse entre as leituras, a
  ferramenta rodava com um `working_dir` diferente daquele a que o "sim" estava
  vinculado, e a pausa era gravada para um destino que a ferramenta nao viu. Agora um
  snapshot preguicoso por turno (que guarda tambem o `None` de uma falha) e
  compartilhado pelos tres pontos nos caminhos batch, streaming e fallback para batch.
  Conversa sem ferramenta continua sem criar diretorio; `contexto_de_ferramenta` segue
  publico. O snapshot e uma string de caminho, nao um handle: nao elimina TOCTOU
  adversarial no sistema de arquivos. Id da chamada, principal escopado por bot e E2E
  real por Telegram/MCP seguem abertos na issue.
