- **`agent.sandbox` passa a conter `run_tests`, `git_diff`, `code_review` e `repo_search` (#1225).**
  Ate aqui so o `bash` consultava a policy: com `mode: all` essas quatro tools
  continuavam spawnando no host. Agora elas passam por um spawn que monta o
  argv do `docker run`/`podman run` sem shell nenhum (argumento hostil do
  modelo chega literal ao programa), com as mesmas recusas fail-closed do
  `bash` (sem backend, binario ausente, fora de unix, imagem com `-`, mount
  invalido) e mais uma: `backend: ssh` e recusado para elas, porque o host
  remoto nao tem o diretorio de trabalho. No timeout o container e removido
  (`rm -f garra-sbx-<uuid>`). O `ssh` + container remoto fica como won't-do:
  `backend: docker` com um `docker context` `ssh://` ja entrega isso.
