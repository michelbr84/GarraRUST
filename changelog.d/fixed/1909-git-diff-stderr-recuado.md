- **`git_diff` nao deixa mais o stderr do git desenhar uma linha da tool
  (#1909).** O git ecoa no stderr o pathspec que recusou, com as quebras de
  linha dele, e o `file_path` e texto do modelo: um
  `:(bogus)x\nIntervalo: ...` escrevia uma linha `Intervalo:` (ou
  `Repositorio:`) forjada na coluna zero, abaixo da verdadeira. O bloco
  `STDERR:` de `diff` e `status` agora sai com cada linha recuada e sem
  caractere de controle, pelo mesmo helper que o `code_review` ja usava
  (movido para `git_endurecido::stderr_recuado`). Muda a forma do bloco: o
  texto do git vem recuado em quatro espacos, e um stderr so de espacos nao
  abre mais o bloco. O stdout (o diff em si) nao e tocado; nenhuma raiz,
  jail, modo ou teto mudou.
