- **As raizes das file tools sao resolvidas uma vez, no boot (#1459).** O
  `/api/diagnostics` (rota auth-free) e o `garra_status` chamavam
  `raizes_das_file_tools` a cada request/chamada: um `canonicalize` por raiz
  declarada e um `warn!` por raiz que nao resolve, amplificaveis por quem
  quisesse. Agora o `AppState` guarda o resultado calculado na subida, da
  mesma config e pela mesma funcao que monta o jail, e as duas superficies
  leem dele — descrevendo o jail que o turno usa, e nao o disco do momento.
  Guardas de fonte impedem a volta do padrao.
