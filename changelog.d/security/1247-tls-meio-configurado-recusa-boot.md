- **TLS configurado pela metade recusa o boot em vez de servir HTTP puro (#1247).**
  Com so `gateway.tls_cert_path` ou so `gateway.tls_key_path` no arquivo, o
  gateway caia em `serve_plain` sem aviso: o operador pediu TLS e recebia
  texto claro, credencial de gateway inclusive. Agora `garraia start`,
  `restart` e `start -d` saem com exit 78 (`EX_CONFIG`) nomeando o campo que
  falta, antes de ligar o socket e antes do fork. Escotilha consciente:
  `GARRAIA_ALLOW_INVALID_CONFIG=1` (exatamente `1`), que sobe e continua
  logando o achado como erro. Quem embute o `GatewayServer` sem a CLI recebe
  um `warn!` nomeando o campo ausente no ponto de uso.
