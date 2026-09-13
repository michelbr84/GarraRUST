- **`gateway.allowed_origins` vazio passa a significar "nenhuma origem cross-origin" (#1182).**
  Antes significava allow-all. Quebra conhecida e deliberada de um perfil: quem serve o
  gateway atras de um reverse proxy com dominio proprio **sem** listar esse dominio em
  `allowed_origins` passa a receber `403` nos `POST`/`PATCH`/`DELETE` vindos do navegador
  (o proxy termina TLS e repassa `Host: meu.dominio`, mas o gateway por baixo fala
  `http`, entao a origem nao bate). A correcao e de uma linha de config: liste o dominio
  em `gateway.allowed_origins` — uma origem declarada ali e aceita como tal, incluindo o
  esquema. Ver `docs/hardening-gateway.md`. O perfil default (loopback, sem chave), que e
  a esmagadora maioria das instalacoes, nao muda.
