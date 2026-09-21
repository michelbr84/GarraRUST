- O REPL interativo (`garra chat` ou `garra`) nao exibe mais linhas cruas de
  tracing (`WARN garraia_agents::...`) no terminal: retry, fallback e circuit
  breaker do provider ficam em silencio no console padrao, porque a falha de
  turno ja vira cartao de erro acionavel. `garraia.log` continua recebendo
  tudo, e `--verbose`, `--debug` ou `RUST_LOG` explicito vencem o silencio
  como antes (#1301).
