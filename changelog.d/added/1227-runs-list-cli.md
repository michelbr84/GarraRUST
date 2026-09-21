- **`garra runs list` le o ledger de runs de agente (#1227).** A tabela
  `agent_runs` ja era povoada pelo scheduler do gateway e ja marcava
  `interrupted` na subida depois de uma queda, mas nao havia como olhar para
  ela sem abrir o SQLite na mao. O comando abre o mesmo `sessions.db` sem
  falar com o gateway — igual ao `garra logs` —, entao ele responde "o que
  estava em voo?" justamente quando o gateway esta fora. `--status`
  (`running`, `done`, `error`, `cancelled`, `interrupted`) filtra no SQL,
  `--limit` corta a janela (padrao 50) e `--json` devolve um array com chaves
  estaveis e instantes em UTC ISO 8601 com `Z`. Ledger vazio nao e erro: sai
  0 com uma linha amigavel (ou `[]`). A listagem e somente leitura e nao
  cria o banco: converter `running` residual em `interrupted` continua sendo
  do hook de subida.
