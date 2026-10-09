- **Direitos dos titulares saem do papel: exportacao completa, correcao de
  e-mail com fluxo seguro e apagamento definitivo rastreavel.** O
  `GET /v1/me/export` devolvia so dado de conta; agora inclui o conteudo
  pessoal do titular em todos os grupos dele — mensagens que escreveu (corpo
  completo), mencoes recebidas (trecho), arquivos enviados, memorias e tarefas
  — numa unica transacao que reaponta a GUC de tenant por grupo, com RLS
  fail-closed e filtro pelo id do titular nas duas pontas. O arquivo passa a
  declarar o que ficou de fora (`exclusions`) e o que bateu no teto
  (`truncated_sections`), porque exportacao incompleta em silencio e
  declaracao falsa de conformidade.
- **`PATCH /v1/me` aceita `email` sem nunca trocar o e-mail as cegas.**
  `users.email` e a chave de login, entao trocar com base so num token de
  acesso e primitiva de account takeover. Como o repo nao tem infraestrutura
  de envio de e-mail, o pedido fica registrado em
  `user_email_change_requests` com estado `pending` + audit, e a aplicacao e
  passo manual do operador com verificacao de posse fora de banda
  (`docs/legal/data-subject-requests.md`). O endpoint tambem nao informa se o
  endereco pedido ja existe — responder isso viraria oraculo de enumeracao de
  contas.
- **`DELETE /v1/me` enfileira o apagamento que antes era "deferido a um worker
  futuro".** A mesma transacao que marca a lapide e revoga as sessoes grava o
  pedido em `account_deletion_requests` com carencia de 30 dias; o novo
  `account_purge_worker` destroi conteudo e identificadores do titular
  (mensagem, comentario, memoria, arquivo + versoes + blob no object store) e
  deixa `users` como lapide `status = 'purged'`. A linha de `users` nao e
  removida porque cinco FKs `NO ACTION` apontam para ela e remove-la apagaria
  contexto de terceiros — mensagem de outro membro nunca e apagada por pedido
  de um. `audit_events` sobrevive desidentificado (`actor_label`, `ip` e
  `user_agent` viram NULL): e a unica prova de que o apagamento aconteceu.
  Sem object store ligado o pedido NAO e fechado — declarar `completed` com
  blob vivo seria mentir no audit.
- **O worker de apagamento roda com credenciais dedicadas e e retomavel.** O
  role `garraia_purge` (migration 034) tem `EXECUTE` nas quatro funcoes
  `SECURITY DEFINER` de apagamento e mais nada — nenhuma privilegio de tabela,
  nao e `BYPASSRLS`; o pool valida `SELECT current_user` igual aos demais
  dedicados. O worker so nasce se `GARRAIA_PURGE_DATABASE_URL` estiver
  definida; sem ela o warn no log e explicito e o procedimento manual em
  `docs/legal/data-subject-requests.md` segue valendo. O progresso (`db_purged`
  + chaves de blob pendentes) e persistido na propria linha do pedido na mesma
  transacao dos deletes: uma queda no meio nao reexecuta a passada de banco nem
  fecha o pedido com blob vivo.
