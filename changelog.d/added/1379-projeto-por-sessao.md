- **Projeto ativo por sessao, persistido, e comandos de barra no WhatsApp
  por principal (#1379, #1424; ADR 0025).** `/project [list|<nome ou
  id>|clear]` seleciona um projeto cadastrado para a conversa: o caminho e
  confinado de novo pelas raizes de projeto do operador
  (`GARRAIA_PROJECT_ROOTS`), vira o `working_dir` das file tools e do
  `repo_search`, e o vinculo fica no `sessions.db` (`sessions.project_id`):
  um `garraia restart` restaura o projeto na hidratacao da sessao — e um
  projeto que ficou fora das raizes nao volta (fail-closed). A resposta
  mostra nome e id curto, nunca o caminho. `POST /api/projects` e
  `GET /api/projects` passam a gravar e ler o banco quando ha `sessions.db`
  (antes viviam so em memoria). No WhatsApp pessoal, uma mensagem que
  comeca com `/` e decidida por principal ANTES de ir ao modelo: `/help`
  para todo admitido, `/project` para dono e usuario, `/mode` e `/goal` so
  para o dono (o nivel dos demais vem da politica de acesso); comando
  registrado fora dessas listas e recusado com motivo; o que nao e comando
  segue como texto. Selecionar projeto nao muda poder: o que se pode fazer
  nele continua sendo o portao do turno (modo e teto do principal).
