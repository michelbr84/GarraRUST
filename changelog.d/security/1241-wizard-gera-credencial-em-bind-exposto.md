- **`garra init` numa VM como root nao deixa mais o gateway na internet sem credencial
  nenhuma (#1241).** O wizard escolhe `gateway.host = 0.0.0.0` sempre que a maquina parece
  server-like — e `is_server_like()` e `is_runpod || is_root`, ou seja, qualquer instalacao
  como root, nao so RunPod. Nesse caminho ele nunca escrevia `gateway.api_key` (os unicos
  `api_key` que o wizard escrevia eram os de `llm.*`), e sem chave o gate de `/api/*`
  devolve todo pedido a `next` na primeira linha: sessoes, memoria, providers, logs e
  diagnosticos abertos a quem alcancasse a porta. Agora, quando o host resolvido nao e
  loopback, o wizard sorteia 32 bytes do CSPRNG do sistema (`garraia_security::random_bytes`,
  o mesmo `ring` do resto do projeto), grava em `gateway.api_key` e imprime a credencial
  **uma unica vez** no resumo final, dentro da linha `curl -H "Authorization: Bearer ..."`
  que o operador vai copiar — nunca em log estruturado. O arquivo ja saia em modo 0600.
  Bind em loopback (o caso do laptop) nao muda em nada: nenhuma chave gerada, nenhuma linha
  nova impressa.
- **Re-rodar o wizard nao derruba mais os clientes ja configurados (#1241).** No caminho de
  merge a credencial gerada so entra quando a do config esta ausente ou em branco — a mesma
  normalizacao que o gate de `/api/*` usa para decidir que nao ha gate. Chave escolhida pelo
  operador nunca e sobrescrita, e o resumo, nesse caso, avisa do bind exposto sem imprimir
  segredo nenhum.
- **O boot avisa quando o bind alcanca a rede e nao ha credencial (#1241).** O gateway so
  logava "listening on". Agora, depois do bind e uma vez por processo, um `warn!` nomeia o
  endereco efetivamente ligado, o que esta aberto e as duas correcoes (`garra init` ou
  `gateway.api_key` no config; `--host 127.0.0.1` para ouvir so localmente). A checagem
  roda sobre o endereco ligado, e nao sobre o arquivo de config, justamente para cobrir o
  override por `--host`/`HOST` do `garra start`, que o `garra config check` nao enxerga.
  O boot **nao** e recusado: quem roda assim de proposito atras de firewall continua
  subindo.
