- **Testes unitarios do `garraia-agents` (`--features mcp --lib`) passam num host Windows (#1697).**
  Nenhum job de CI executava testes unitarios fora do Linux, e a suite acumulou falhas
  invisiveis no Windows: `termux_ld_preload` montava o valor de `LD_PRELOAD` (um caminho
  POSIX lido pelo loader do Android) com `Path::join`, que no Windows poe `\`; a ordem de
  resolucao de `npm_cache_root` tratava "absoluto" pela plataforma do host em vez da do
  filho simulado; os testes do sandbox que passam `"/tmp"` como cwd (semantica Unix de
  montar o cwd num container, fail-closed por desenho em host Windows) ganham
  `cfg(not(windows))`; e um helper de teste usado so por testes `cfg(not(windows))` ganha
  o mesmo cfg, zerando o `dead_code` que o clippy do Windows apontava. Comportamento em
  Linux inalterado. Prepara o CI a executar `--lib` no Windows/macOS.
