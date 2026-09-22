- **`garraia start` recusa subir num bind exposto sem credencial de gateway (#1261).**
  **Quebra de compatibilidade para deploys expostos sem chave:** `start`,
  `start -d` e `restart` saem com exit 78 (`EX_CONFIG`) quando qualquer
  endereco resolvido do bind nao e loopback e nao ha `gateway.api_key` nem
  `GARRAIA_GATEWAY_API_KEY`. Antes o boot so avisava e subia com o `/ws` (tools
  de arquivo e de dispositivo) e o `/api/mcp/marketplace/install` abertos para
  quem alcancasse a porta. A recusa vem antes do bind, antes do fork em `-d`
  (a mensagem chega ao terminal) e antes de o `restart` derrubar o daemon
  atual; TLS nao isenta, e um nome que nao resolve tambem e recusado. A
  mensagem diz como corrigir usando o binario instalado: `garraia init`,
  `gateway.api_key`, `GARRAIA_GATEWAY_API_KEY` ou `--host 127.0.0.1`. Deploy
  aberto de proposito atras de proxy que autentica tem o opt-out
  `gateway.allow_unauthenticated_network_bind: true`, so no arquivo (sem env
  nem flag, para que a mesma injecao de `HOST` que expoe o bind nao desligue
  a guarda), com aviso alto em todo boot. **Migracao (faca ANTES de subir a
  imagem v0.4.5, `latest` incluso):** a imagem Docker, os tres
  `docker-compose*.yml` (o `.env.example` traz a linha vazia, para preencher) e os pods RunPod
  precisam de `GARRAIA_GATEWAY_API_KEY` no ambiente (`openssl rand -hex 32`);
  sem ela o container sai com 78 e o `restart: unless-stopped` o reinicia em
  loop. O chart Helm ganhou `gatewayApiKey` (Secret proprio, gerado no install
  e preservado no upgrade, ou `existingSecret`/`value`; `helm template`/ArgoCD
  precisam de um dos dois) e recusa renderizar a chave via `secretEnv`. O
  modulo Terraform/ECS ganhou a variavel **obrigatoria**
  `gateway_api_key_secret_arn` (Secrets Manager ou SSM), injetada como
  `GARRAIA_GATEWAY_API_KEY`, e a role de execucao passou a ler os ARNs de
  `secrets` — um `terraform plan` sem ela falha antes de trocar a imagem.
  Instalacoes do `install.sh` e a unit systemd ligam em loopback e nao mudam.
  O desktop, o cookie de sessao (`Secure`), o alerta do admin e o hot-reload
  do `config.yml` passam a enxergar a chave que so vem da env. Nova env `GARRAIA_GATEWAY_API_KEY`:
  vence o arquivo, vazia conta como ausente, e mora num campo que o serde
  nunca le nem grava, entao um `save()` depois do `load()` nao a escreve no
  `config.yml`. `garraia config check` passa a dizer **Error** onde o `start`
  recusaria, e reporta a env so por presenca.
