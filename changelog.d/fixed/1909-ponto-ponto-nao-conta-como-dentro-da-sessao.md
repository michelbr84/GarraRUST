- **Caminho que sai do diretorio da sessao por `..` deixa de aparecer como
  relativo no turno restrito (#1909).** A comparacao com o `working_dir` era
  so lexica: `<wd>/../vizinho/x` comecava pelo diretorio da sessao e saia como
  `../vizinho/x` nas mensagens de tool e no stderr do git repassado, nomeando
  um diretorio irmao. Um resto com `..` agora conta como fora, e no turno
  restrito sai o marcador. Nenhuma raiz, jail ou modo mudou; e so o texto
  exibido.
