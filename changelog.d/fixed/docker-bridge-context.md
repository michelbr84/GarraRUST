- Imagem Docker: o estagio builder passa a copiar `bridge/` — o
  `garraia-channels` embute o bridge WhatsApp com `include_str!` e o `cargo
  build` dentro da imagem falhava com "couldn't read bridge/whatsapp/..."
  (foi o que derrubou o Deploy do tag v0.4.3). Um teste em `garraia-channels`
  prende a regra: todo diretorio fora de `crates/` que um `include_str!` de
  producao alcanca tem de estar no Dockerfile. O `deploy.yml` disparado a mao
  com `tag=vX.Y.Z` passa a produzir tambem as tags `X.Y.Z` e `X.Y`, e ganha o
  input `latest` para mover a tag `latest` so quando pedido.
