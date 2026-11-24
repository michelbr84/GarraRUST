- **`code_review` passa a dizer o intervalo exato que revisou (#1909).** O
  `git_diff` ja abria com a linha `Intervalo:`; a revisao por LLM seguia
  ecoando so o `commit_range` como texto, e `main..feature` e nome movel. O
  cabecalho da revisao agora traz `Intervalo: <de> (<SHA completo>) .. <ate>
  (<SHA completo>)` para `A..B` e `A^!`, `base comum de A (...) e B (...) .. B
  (...)` para `A...B`, `A (...) .. arvore de trabalho` para uma revisao so, e
  indice contra arvore de trabalho (com o `HEAD`) sem `commit_range`. As pontas
  sao resolvidas antes e o diff roda sobre os SHAs. Mudam dois comportamentos:
  ponta que nao e commit do repositorio recusa a revisao dizendo qual faltou, e
  um `git diff` que termina com erro (duas pontas sem base comum) sai como
  falha — antes os dois casos voltavam como "No diff output (no changes
  found)". O `-` inicial passa a ser recusado em cada ponta, nao so no comeco
  do `commit_range`. O `commit_range` e o `file_path` ecoados no cabecalho saem em
  uma linha so. O teto declarado da tool sobe de um para tres timeouts de git
  mais o do LLM. So leitura (`git rev-parse --verify`, mesmo endurecimento
  do diff); nenhuma raiz, jail ou modo mudou.
