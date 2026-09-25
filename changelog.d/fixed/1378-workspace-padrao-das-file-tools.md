- **Sessao sem projeto ganha um workspace seguro em vez de nenhuma raiz (#1378).** Uma
  sessao do WhatsApp recem-vinculada nasce com `working_dir = null`. Com
  `agent.file_roots` vazio — o default de toda instalacao limpa — o conjunto de raizes
  do jail das file tools ficava vazio, e vazio significa negar tudo (#1244): `file_read`,
  `file_write` e `list_dir` apareciam registradas, o modo as anunciava, e toda chamada
  voltava recusada. A capability existia no papel e nao existia na pratica. Agora, quando
  nada foi declarado, a raiz default e `<data_dir>/workspace/<sessao>` — um subdiretorio
  **por sessao** dentro do endereco que o ADR 0024 ja nomeia como o workspace do proprio
  Garra —, igual nos dois perfis de execucao:
  `execution.pod_root` muda so a raiz do servidor MCP `filesystem` e **nao** e herdado
  pelas file tools nativas. O boot cria esse diretorio — e **so** ele: uma raiz
  declarada com typo continua nao sendo materializada, para nao plantar diretorio no host
  por efeito colateral da subida. **Nunca** `/` e **nunca** o `$HOME`: um caminho mais
  largo segue sendo escolha explicita do operador. Nada muda para quem declarou raiz em
  `agent.file_roots` ou em `GARRAIA_FILE_ROOTS` — a declaracao vence e o default nem e
  consultado —, caminho fora da raiz continua recusado com a mesma mensagem unica, e se
  o workspace nao puder ser criado o jail volta ao fail-closed anterior, com aviso no
  boot. O `/api/diagnostics` passou a trazer a linha `files.workspace`, que diz qual e o
  workspace efetivo e **por que** ele e esse (declaracao do operador, workspace padrao,
  ou nenhuma raiz). O workspace novo nasce `0700` (so o dono), um symlink plantado no
  lugar dele e recusado em vez de seguido — senao o jail herdaria o alvo do link, que
  poderia ser justamente `/` ou o `$HOME` —, e a linha do `/api/diagnostics` relativiza
  o caminho tambem quando o `data_dir` passa por symlink, onde antes ela caia no
  fallback e imprimia o caminho absoluto do host numa rota auth-free. Vale notar que
  este default tambem passa a alcancar a `RunTestsTool`: a combinacao `file_write` +
  `run_tests` que a #1272 (SANDBOX-1) marca como adjacente ao shell do host agora tem
  um diretorio onde operar, dentro do jail.

  **O workspace padrao e isolado por sessao (#1449).** A primeira versao desta correcao
  punha `<data_dir>/workspace` como raiz **fixa** do jail, e raiz fixa e a mesma para
  toda sessao, canal e principal: um contato do WhatsApp escrevia ali e o turno de outro
  lia. Isso e disclosure cross-principal, e tambem um vetor persistente de injecao
  indireta — conteudo escrito por um principal voltando no contexto de outro sem passar
  pelo guard de entrada. Agora o workspace padrao nao e raiz do jail: ele e o **pai** de
  um subdiretorio por sessao, e e esse subdiretorio que entra como `working_dir` da
  chamada, pelo mesmo mecanismo (`session_dir`) que uma sessao com projeto ja usava desde
  a #1244. No caso "workspace padrao" o jail fica literalmente sem raiz fixa, entao a
  sessao A nao tem como alcancar o diretorio da sessao B — nem o pai, que enumeraria as
  sessoes existentes. O nome do subdiretorio e um hash do identificador da sessao, e nao
  o identificador: hashear e o que impede tanto `..`/separador de caminho vindos de um id
  nao confiavel quanto gravar em disco (e no log) um identificador que no WhatsApp e o
  contato. O diretorio de cada sessao nasce preguicosamente, fecha em `0700`, e recusa
  symlink no lugar dele — inclusive no pai, verificado de novo no momento do uso —, caindo
  no mesmo fail-closed de sempre em vez de herdar o alvo do link. Raiz declarada em
  `agent.file_roots`/`GARRAIA_FILE_ROOTS` continua vencendo sozinha, **sem** escopo por
  sessao: um diretorio compartilhado ali e escolha explicita do operador. A linha
  `files.workspace` do `/api/diagnostics` passa a mostrar `<data_dir>/workspace/<sessao>`,
  sem nunca publicar o identificador da sessao.

  O `working_dir` sintetizado alcanca toda invocacao de ferramenta, nao so as quatro file
  tools: `bash` (sandboxed), `git_diff`, `code_review` e `repo_search` tambem passam a
  operar dentro do diretorio da sessao. No `bash` sandboxed isso troca a recusa
  fail-closed de antes (mount vazio sem `working_dir`, #1272 SANDBOX-2/5) por execucao
  de verdade dentro de um diretorio novo e vazio; fora do sandbox, e nas outras tres
  tools, o efeito e estritamente mais estreito — antes caiam no CWD do processo do
  gateway, que expunha o checkout onde o gateway roda.
