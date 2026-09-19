- **O `git_diff` e o `code_review` respondiam sobre o repositorio errado.** As duas tools
  montavam o `Command::new("git")` sem `current_dir`, entao o git herdava o diretorio de
  trabalho do *processo do gateway* em vez do `working_dir` da sessao que pediu a tool: uma
  sessao com projeto em A recebia o diff do repositorio onde o gateway subiu — ou
  "not a git repository", quando aquele diretorio nao era repositorio nenhum. Pior que a
  resposta errada, o resultado nao era reproduzivel entre instalacoes: o diretorio do
  processo depende de como ele subiu (`garra start` num terminal, unidade systemd com
  `WorkingDirectory=`, sidecar do desktop, container), de modo que o mesmo prompt, na mesma
  sessao, dava resposta diferente sem nada no pedido explicar a diferenca. Agora o git roda
  no `working_dir` da sessao quando ha um; sem ele — o caso comum, porque um prompt de canal
  nao traz projeto — o diretorio do processo e mantido, mas a resposta passa a **dizer de
  qual repositorio ela falou**, porque a resposta errada silenciosa era o defeito real.
  Um `working_dir` que nao existe falha nomeando o diretorio em vez de cair de volta no
  diretorio do processo. Os testes exercitam as tools pelo caminho do agente (`execute` com
  `ToolContext`) contra dois repositorios temporarios, e nao pela funcao interna. No
  `code_review`, de carona e no espirito do #1269, o `commit_range` (que vem do modelo e e
  argumento argv) passou a ser recusado quando comeca com `-` (`--output=...` escreveria
  arquivo, `--stdin` penduraria o filho ate o timeout) e o filho git nao le mais a entrada
  padrao do gateway.
