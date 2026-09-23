- **Sessao sem projeto ganha um workspace seguro em vez de nenhuma raiz (#1378).** Uma
  sessao do WhatsApp recem-vinculada nasce com `working_dir = null`. Com
  `agent.file_roots` vazio — o default de toda instalacao limpa — o conjunto de raizes
  do jail das file tools ficava vazio, e vazio significa negar tudo (#1244): `file_read`,
  `file_write` e `list_dir` apareciam registradas, o modo as anunciava, e toda chamada
  voltava recusada. A capability existia no papel e nao existia na pratica. Agora, quando
  nada foi declarado, a raiz default e a mesma que o ADR 0024 ja dava ao servidor MCP
  `filesystem`: `<data_dir>/workspace` em `standard`, `execution.pod_root` (ou o mesmo
  workspace) em `isolated-pod`. O boot cria esse diretorio — e **so** ele: uma raiz
  declarada com typo continua nao sendo materializada, para nao plantar diretorio no host
  por efeito colateral da subida. **Nunca** `/` e **nunca** o `$HOME`: um caminho mais
  largo segue sendo escolha explicita do operador. Nada muda para quem declarou raiz em
  `agent.file_roots` ou em `GARRAIA_FILE_ROOTS` — a declaracao vence e o default nem e
  consultado —, caminho fora da raiz continua recusado com a mesma mensagem unica, e se
  o workspace nao puder ser criado o jail volta ao fail-closed anterior, com aviso no
  boot. O `/api/diagnostics` passou a trazer a linha `files.workspace`, que diz qual e o
  workspace efetivo e **por que** ele e esse (declaracao do operador, workspace padrao,
  ou nenhuma raiz).
