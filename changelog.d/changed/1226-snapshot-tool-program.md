- **O snapshot de capacidades do `garraia max-power` passa a anunciar o
  `tool_program` (#1226).** A lista estatica de tools do snapshot nao tinha a
  ferramenta intrinseca que o runtime ja expoe nos modos `auto`, `code` e
  `ask`, e o teste que devia fixa-la so comparava o tamanho da lista com ele
  mesmo. Agora o teste fixa a lista inteira, em ordem. As tools `device_*` e
  `schedule_*` continuam fora de proposito: so existem em algumas instalacoes.
  Em `docs/src/modes.md` fica registrada a decisao de nao incluir
  `tool_program` na whitelist dos perfis nativos `search`, `architect`,
  `debug`, `orchestrator`, `review` e `edit`; um perfil customizado pode
  liga-la.
