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
  a guarda), com aviso alto em todo boot. **Migracao:** a imagem Docker, o
  `docker-compose.yml` e os pods RunPod precisam de `GARRAIA_GATEWAY_API_KEY`
  no ambiente (`openssl rand -hex 32`); instalacoes do `install.sh` e a unit
  systemd ligam em loopback e nao mudam. Nova env `GARRAIA_GATEWAY_API_KEY`:
  vence o arquivo, vazia conta como ausente, e mora num campo que o serde
  nunca le nem grava, entao um `save()` depois do `load()` nao a escreve no
  `config.yml`. `garraia config check` passa a dizer **Error** onde o `start`
  recusaria, e reporta a env so por presenca.
