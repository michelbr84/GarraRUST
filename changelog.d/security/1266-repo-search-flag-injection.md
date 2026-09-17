- **A query do modelo na `repo_search` deixa de cair em posicao de flag — o
  ripgrep nao executa mais comando escolhido por prompt (correcao da #1266).**
  A tool entregava a `query` da tool call como argumento posicional, sem o
  terminador `--`, nos tres comandos que ela monta: `rg`, e os fallbacks `grep`
  (Unix) e `findstr` (Windows). Como o ripgrep aceita opcao em qualquer
  posicao, o texto que o modelo escolhe era lido como opcao: `--pre=/bin/sh`
  faz o proprio `rg` rodar um pre-processador para cada arquivo varrido, ou
  seja, executa como script um `.txt` comum — daqueles que a `file_write` pode
  gravar legitimamente dentro do jail de diretorio. A cadeia inteira passa por
  fora do `bash_tool`, entao nem o allowlist de comando nem o sandbox dele
  participam, e o piso somente-leitura de canal nao cobre porque `repo_search`
  e ferramenta de leitura. No `grep`, a mesma posicao rende `-f/etc/passwd`, que
  troca a busca pedida por padroes lidos de um arquivo escolhido pelo modelo.
  Agora cada comando e montado por uma funcao pura que poe a `query` **depois**
  do `--`. O `findstr` nao tem terminador — qualquer argumento comecando com
  `/` vira opcao — entao ali a query vai em `/C:<texto>`, a forma documentada
  para string de busca literal; o custo e que esse fallback perde regex, o que
  vale menos que a classe de injecao. O `file_pattern`, que tambem vem do
  modelo, passou a `--glob=<valor>`, colado ao nome da opcao em vez de
  depender de como cada versao do ripgrep resolve um valor que comeca com `-`.
  Uma query legitima que por acaso comece com `-` (`-> foo`) continua
  funcionando: e buscada literalmente, em vez de recusada.
- **A `repo_search` nao consome mais a entrada padrao de quem a chamou
  (#1266).** Os filhos `rg`/`grep` herdavam o stdin do gateway. Sem argumento
  de caminho e com um pipe no stdin, o ripgrep decide **buscar no stdin** em
  vez de varrer o diretorio, e fica pendurado ate o timeout de 15s comendo a
  entrada do processo pai. Agora os dois comandos sobem com `Stdio::null()`,
  o que tambem torna o comportamento identico em terminal, pipe e servico —
  sem isso, o teste de regressao da injecao passava em CI por acidente, porque
  o `cargo test` da um pipe ao filho e o ataque nem chegava a rodar.
