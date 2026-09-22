- **Com `agent.sandbox.mode: all`, a imagem do sandbox precisa da toolchain das tools de repositorio (#1225).**
  `run_tests`, `git_diff`, `code_review` e `repo_search` rodam no container.
  A imagem default (`debian:bookworm-slim`) so tem `grep`: `repo_search` cai
  para ele dentro do container, e `run_tests`/`git_diff`/`code_review`
  respondem que o programa nao existe na imagem (so quando o proprio runtime
  diz isso no stderr: um exit 127 do script de teste devolve a saida real da
  suite). Para migrar, aponte
  `agent.sandbox.image` para uma imagem com `git`/`cargo`/`rg`, ou liste a
  tool em `agent.sandbox.elevated` para ela seguir no host. `mode: off` (o
  default) nao muda nada. O aviso de subida e o `garraia config check`
  deixaram de dizer que essas tools rodam no host.
