- **`garra chat --mode` e `/mode` (#1858, primeira fatia).** O REPL do CLI
  ganha os mesmos modos de execucao do gateway e do Web Console (`search`,
  `architect`, `code`, `ask`, `debug`...), aplicados pelo mesmo `ToolGate` do
  runtime via `ExecContext::agent_mode`: `search`/`architect` sao somente
  leitura. `/mode` mostra o atual, `/mode <nome>` troca, `/mode off` volta ao
  "sem politica"; `/status` mostra o modo. Sem a flag nada muda. Nome
  desconhecido e erro, nunca portao aberto em silencio (`auto` avisa que roda
  sem politica quando a heuristica nao decide, como no gateway). O prompt do modo entra
  depois do prompt do CLI, que carrega o diretorio de trabalho. So restringe
  ferramentas: o tier de confirmacao do bash e o hard-deny ficam intocados — o
  nivel de aprovacao (`yolo`) segue em aberto na issue.
