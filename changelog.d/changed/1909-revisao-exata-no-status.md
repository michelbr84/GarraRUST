- **`git_diff` (`operation: "status"`) nomeia a revisao exata do checkout
  (#1909).** O status dizia o ramo e os arquivos modificados, mas nao QUAL
  commit: num worktree de revisao, "Branch: main" nao distingue duas arvores, e
  o fluxo remoto de revisao ficava sem identidade de checkout — no dogfood do
  Telegram o agente ofereceu revisar um worktree e nao havia como saber que
  revisao seria lida. Agora a resposta traz `Revisão: <SHA completo do HEAD>`
  logo depois do ramo, lida no MESMO diretorio do `status` (o `working_dir` da
  sessao), e a revisao so aparece se a forma for de SHA — texto de erro do git
  nunca vira revisao. Repositorio ainda sem commit nao derruba a tool: a linha
  diz que a revisao esta indisponivel e o resto do estado continua na resposta.
