- **Resposta de `git_diff`/`code_review` e pedido de confirmacao do `run_tests`
  deixam de entregar o caminho do host no turno restrito (#1909).** As duas
  superficies da familia da #1936 ficaram de fora daquele portao: a linha
  `Repositorio:` nomeava o CWD do processo do gateway sempre que a sessao nao
  tinha `working_dir` (o caso comum no Telegram), e o pedido de confirmacao do
  `run_tests` mostrava o diretorio resolvido absoluto. Num turno restrito o
  caminho agora ocupa o lugar do marcador `<outside the session's directory>`
  (ou a forma relativa, quando o alvo cai dentro do `working_dir` da sessao),
  pelo mesmo `pode_revelar_caminho_do_host`; no pedido do `run_tests` o
  marcador vem acompanhado do **pedido** (o argumento do modelo), para a
  aprovacao nao ficar cega. A procedencia continua nomeada (#1258) e a
  impressao digital da aprovacao continua presa ao caminho REAL — nenhuma
  raiz, jail, modo ou teto foi alterado, e o turno aberto do operador local ve
  o absoluto como antes.
  Limite conhecido, herdado do portao: "turno restrito" e a **forma da
  politica de tools** (allowlist ou teto por principal), nao a identidade do
  remetente — um modo sem allowlist, vindo de canal remoto e sem teto por
  principal, segue contando como aberto e mostra o caminho.
