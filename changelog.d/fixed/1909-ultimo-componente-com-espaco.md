- **Ultimo componente com espaco nao fica mais solto no stderr do git em
  turno restrito (#1909).** Sem aspas, `/home/dono/Meus Projetos` saia como
  `<marcador> Projetos` no corpo de `git_diff`/`code_review` quando o git
  citava o repositorio da sessao. O caminho agora sai inteiro quando e o
  `working_dir` da sessao ou um ancestral dele. O disco nao e consultado, para
  nao virar oraculo de existencia; fora dessa linhagem segue o limite antigo.
  Nenhuma raiz, jail ou modo mudou; e so o texto exibido.
