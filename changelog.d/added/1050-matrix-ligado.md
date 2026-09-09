- canais: o Matrix passa a ser registrado quando ha uma secao
  `[channels.<nome>]` com `channel_type = "matrix"`. O `MatrixChannel` tinha
  `impl Channel` e sync loop desde antes, e nenhum call-site. `garra config
  check` ganha a env var que faltava para o token (`MATRIX_ACCESS_TOKEN`, sem
  a qual a falta dele saia em silencio) e um aviso para `homeserver_url`
  ausente, que nao e credencial e por isso escapava da checagem generica.
  (#1050)
