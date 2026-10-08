- **Allowlist do bash avalia encadeamento segmento a segmento (#1592).** A allowlist
  do `bash` negava QUALQUER comando com `|`, `&&`, `||` ou `;` — era a opcao
  fail-closed de #1105, e por isso nunca houve bypass, mas derrubava o pipeline
  legitimo: com `git *` e `grep *` declarados, um `git status | grep modificado`
  morria no tier arriscado. Agora os separadores de estrutura sao quebrados em
  segmentos (varredura ciente de aspas, a mesma de #1579) e a lista confere CADA
  segmento: o comando casa so quando TODOS casam. `git status | sh` segue negado
  enquanto `sh` nao estiver declarado, e um pipeline nao concede autoridade que o
  operador ja nao tenha dado — `a | b` com os dois declarados equivale a duas
  chamadas de tool seguidas, que o modelo ja podia fazer. O que NAO foi afrouxado:
  substituicao de comando (`$(...)`, crase), redirecionamento (`<`, `>`), `&` solto
  (background escapa do timeout e da supervisao), nova linha e separador
  pendurado/colado continuam negando o comando inteiro, porque nenhum deles e um
  segundo comando que se possa conferir contra a lista. A denylist do `safety_gate`
  segue na frente de tudo. Mensagem de negacao e prompt do `HostComAllowlist`
  atualizados para a regra nova.
