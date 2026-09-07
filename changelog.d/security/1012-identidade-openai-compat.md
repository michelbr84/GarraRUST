- **`/v1/chat/completions` para de derivar identidade do que o chamador
  escreve (#1012).** A rota e auth-free por desenho, como todo o `/api/*` — mas
  auth-free significa "nao exige credencial", e nao "aceita a identidade que o
  chamador afirmar". Ela aceitava as duas coisas que vem do proprio chamador:
  um `Authorization: Bearer` qualquer virava o `user_id` (o comentario no fonte
  dizia *"this allows custom API keys to identify users"*, o que nunca foi
  verdade — a rota nao verifica o token contra nada, entao equivalia a deixar
  o chamador escolher o proprio nome), e na ausencia dele o header `X-User-Id`
  era usado cru.
- **Severidade: latente, nao ativa.** Rastreado ate o fim, o `user_id` nao abria
  leitura de dado alheio — a carga de historico e chaveada por `session_id`. O
  impacto era **atribuicao falsa**: a sessao e o registro no banco ficavam sob
  uma identidade que ninguem provou. Vale fechar porque e a mesma forma do
  buraco que a #1010 fechou de proposito *antes* de ligar execucao nele.
- **Agora a identidade e a do dono da instalacao local, ou `None`.** `None` e a
  resposta honesta para instalacao sem dono: preenche-la com o header seria
  inventar um dono. O `garra-local` continua resolvendo o dono, com teste de
  nao-regressao. Um bearer que nao seja `garra-local` e ignorado, com apenas o
  fingerprint no log — nunca o token.
- **`AppState::continuity_key` perdeu o parametro que nunca usava.** Ele
  recebia `_user_id` e quatorze chamadores passavam identidade real (Telegram,
  Slack, WhatsApp, Discord, iMessage) recebendo a mesma `bus:shared-global` de
  volta; o `_` era a unica coisa separando o leitor da conclusao errada de que
  a chave era escopada por pessoa. O parametro foi **removido** em vez de
  passar a ser honrado: honra-lo mudaria o significado de
  `memory.shared_continuity` para quem ja o ligou, e "shared" e o que a opcao
  promete. O barramento e global por desenho, e agora a assinatura diz isso.
- Postura da rota documentada em `docs/security/threat-model.md` §5.9.
