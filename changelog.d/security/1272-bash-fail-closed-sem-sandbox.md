- **O `bash` do `garraia mcp-server` e do gateway deixa de rodar no host em `standard` (#1272).**
  Em `execution.profile = standard` a tool `bash` so e registrada quando
  `agent.sandbox` a coloca dentro de um container `docker` ou `podman` que
  existe no host (todo comando passa por `wrap_command`, sem fallback para o
  host); em `execution.profile = isolated-pod` explicito ela roda no host do
  pod, com a denylist e o tier arriscado ligados. Em qualquer outro caso
  (sandbox desligado, o default; `ssh`, que e execucao remota; `bash` em
  `elevated`; `allowlist` sem `bash`; binario ausente) ela simplesmente nao
  existe: antes, um `cat /etc/shadow` ou um `echo x > /qualquer/lugar` pedido
  pelo modelo rodava no host, porque o tier arriscado so pega o que parece
  perigoso. A decisao nunca vem de detectar container. Instalacoes existentes
  continuam subindo: o boot do gateway e do `garraia mcp-server` avisa uma vez
  com `warn!` dizendo por que e como religar, `/api/diagnostics` ganha o check
  `tools.bash`, e o system prompt do `garra_agent` diz ao modelo que nao ha
  shell. `garraia chat` (humano no terminal, com confirmacao) nao muda.
  A regra vale para toda tool que executa codigo do repositorio: o
  `run_tests` do gateway (que roda o `scripts.test` do `package.json`, o
  `build.rs` e o `conftest.py` que o `file_write` consegue escrever) tambem
  nao e registrado em `standard` sem sandbox. Em `isolated-pod` com sandbox
  exigido e inutilizavel a tool fica de fora, em vez de ser anunciada como
  rodando no host do pod.
- **O container do sandbox passa a ser fronteira de verdade (#1272).** `docker
  run` ganha `--cap-drop ALL`, `--pids-limit 512` e `--user <uid>:<gid>` do
  operador (`podman` usa `--userns=keep-id`), entao nada que o comando deixa
  no diretorio montado pertence a root nem e device. O mount e o caminho
  canonico e absoluto do diretorio de trabalho da sessao; relativo,
  inexistente, com `:`, ausente (sessao sem `working_dir`: o cwd do processo
  nunca e montado no lugar), `/`, o `$HOME` ou um ancestral dele agora e
  recusa fail-closed, em vez de rodar sem o mount ou com o disco inteiro.
- **`git_diff` e `code_review` nao executam programa plantado no repositorio
  (#1272).** O git dessas tools roda com `core.fsmonitor=false`,
  `safe.bareRepository=explicit`, `core.hooksPath=/dev/null`, `--no-textconv`,
  todo `filter.<driver>` da config anulado, nenhuma descida em submodulo
  (`--ignore-submodules=all`, `diff.submodule=short`,
  `submodule.recurse=false`: um submodulo tem config propria) e
  `GIT_CONFIG_NOSYSTEM=1`; e
  `file_write` recusa qualquer caminho com componente `.git`, para o modelo
  nao plantar a config que o proximo diff executaria no host.
