- **`garra init` numa VM como root nao deixa mais o gateway na internet sem credencial
  nenhuma (#1241).** O wizard escolhe `gateway.host = 0.0.0.0` sempre que a maquina parece
  server-like — e `is_server_like()` e `is_runpod || is_root`, ou seja, qualquer instalacao
  como root, nao so RunPod. Nesse caminho ele nunca escrevia `gateway.api_key` (os unicos
  `api_key` que o wizard escrevia eram os de `llm.*`), e sem chave o gate de `/api/*` e do
  `/ws` devolve todo pedido a `next` na primeira linha: sessoes, memoria, providers, logs,
  diagnosticos e o canal do agente com as tools abertos a quem alcancasse a porta. Agora,
  quando o host resolvido nao e loopback, o wizard sorteia 32 bytes do CSPRNG do sistema
  (`garraia_security::random_bytes`, o mesmo `ring` do resto do projeto) e grava em
  `gateway.api_key`. Bind em loopback (o caso do laptop) nao muda em nada: nenhuma chave
  gerada, nenhuma linha nova impressa.
- **A credencial gerada nao e impressa no terminal (#1241).** O resumo do wizard diz em que
  arquivo e em que campo ela esta, e a linha de `curl` sai com um placeholder no lugar do
  segredo. Imprimir era conveniencia e nao necessidade — a chave acabou de ser gravada no
  `config.yml` do proprio operador —, e o custo era real: scrollback do terminal, `tee` ou
  pipe da saida do `init`, captura de stdout em automacao. O CodeQL apontou esse fluxo
  (`rust/cleartext-logging`) e estava certo; a correcao foi parar de imprimir, sem entrada
  nova no ledger de supressoes. A mensagem anterior tambem afirmava "guarde agora, ela nao
  e impressa de novo", o que era falso: a chave esta no arquivo, e acreditar nisso levava
  justamente a re-rodar o wizard por panico.
- **O `config.yml` e criado ja em modo 0600 (#1241).** Antes ele nascia com
  `0666 & ~umask` — normalmente 0644 — ja contendo a credencial, e so depois
  `harden_secret_file` fazia o `chmod`. Entre as duas coisas havia uma janela em que
  qualquer usuario da maquina podia ler o segredo. Agora o modo entra no proprio `open(2)`;
  o `harden_secret_file` continua como cinto-e-suspensorio, porque no caminho de merge o
  arquivo ja existia e `mode` so vale na criacao.
- **Re-rodar o wizard e escolher "Merge / update" nao derruba os clientes ja configurados
  (#1241).** Nesse caminho a credencial gerada so entra quando a do config esta ausente ou
  em branco — a mesma normalizacao que o gate de `/api/*` usa para decidir que nao ha gate.
  Chave escolhida pelo operador nunca e sobrescrita ali, e o resumo, nesse caso, avisa do
  bind exposto sem imprimir segredo nenhum. **A opcao "Backup the existing config and write
  a new one", que e o default do menu, tem o comportamento oposto e sempre teve:** o config
  e reconstruido do zero, entao uma `gateway.api_key` que ja existia e substituida pela nova
  e os clientes antigos param de autenticar. O resumo agora diz isso em uma linha, e o valor
  anterior continua no arquivo `.bak-` gerado ao lado.
- **O boot avisa quando o bind alcanca a rede e nao ha credencial (#1241).** O gateway so
  logava "listening on". Agora, depois do bind e uma vez por processo, um `warn!` nomeia o
  endereco efetivamente ligado, o que esta aberto (`/api/*` e tambem o `/ws`, que e o canal
  do agente com as tools) e as duas correcoes (`garra init` ou `gateway.api_key` no config;
  `--host 127.0.0.1` para ouvir so localmente). A checagem roda sobre o endereco ligado, e
  nao sobre o arquivo de config, justamente para cobrir o override por `--host`/`HOST` do
  `garra start`, que o `garra config check` nao enxerga. Em `garra start -d` e
  `restart -d` o mesmo texto sai em stderr **antes** do fork: depois dele o tracing aponta
  para `~/.garraia/garraia.log` e o aviso nunca chegaria ao terminal — justamente no modo
  que o `install.sh` recomenda e que uma unit systemd usa. O boot **nao** e recusado: quem
  roda assim de proposito atras de firewall continua subindo.
- **`garra config check` e o Web Console pararam de dar falsa garantia sobre a credencial
  (#1241).** Os dois reportavam pela presenca do campo (`is_some()`), entao um
  `api_key: "  "` aparecia como configurado enquanto o gate estava desligado — falsa
  garantia exatamente para quem foi consultar o diagnostico. A normalizacao agora mora num
  lugar so, `GatewayConfig::api_key_normalizada`, e o gate, o `config check` e o
  `GET /api/settings/effective` leem dela.
- **O exemplo minimo do `garra init` sem TTY passou a incluir a secao `gateway` (#1241).**
  Esse e o caminho de container e CI, que e onde `HOST=0.0.0.0` tem mais chance de estar
  setado e onde nao ha wizard para mintar credencial nenhuma.
