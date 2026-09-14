- **`gateway.allowed_origins` vazio passa a significar "nenhuma origem cross-origin" (#1182).**
  Antes significava allow-all. Quebra conhecida e deliberada de um perfil: alcancar o
  Web Console por **nome DNS** — reverse proxy com dominio proprio, mDNS (`nas.local`),
  Tailscale MagicDNS, nome de servico Docker/Compose, ingress — **sem** listar esse nome
  em `allowed_origins` passa a receber `403` nos `POST`/`PATCH`/`DELETE` vindos do
  navegador e no handshake do chat (`/ws`), porque o nome nao atravessa a ancora
  anti-DNS-rebinding (no reverse proxy, alem disso, o proxy termina TLS e o gateway por
  baixo fala `http`, entao o esquema da origem nao bate). A correcao e de uma linha de
  config: liste a origem em `gateway.allowed_origins` (`https://garraia.seudominio.com`,
  `http://nas.local:3888`) — uma origem declarada ali e aceita como tal, incluindo o
  esquema, e destrava CORS, guarda e WebSocket de uma vez. Acesso por IP ou `localhost`
  nao precisa de nada. Ver `docs/hardening-gateway.md`. O perfil default (loopback, sem
  chave), que e a esmagadora maioria das instalacoes, nao muda.
