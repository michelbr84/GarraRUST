- **`repo_search` nao deixa mais o stderr do `rg`/`grep` desenhar uma linha
  na coluna zero (#1909).** O `rg` e o `grep` ecoam a `query` do modelo no
  erro de regex, com as quebras de linha dela: um `(\nRepositorio: ...`
  aparecia cru depois de `STDERR: `, como linha propria. O bloco `STDERR:`
  da busca (ramo do `rg` e erro do fallback `grep`/`findstr`) agora passa
  pelo mesmo `git_endurecido::stderr_recuado` do `git_diff`: cada linha
  recuada em quatro espacos e sem caractere de controle, e um stderr so de
  espacos nao abre o bloco. Muda a forma do bloco (`STDERR:` em linha
  propria, texto recuado embaixo); o stdout com os resultados nao e tocado e
  nenhuma raiz, jail, modo ou teto mudou.
