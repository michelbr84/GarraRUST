- A description do agente test-engineer no frontmatter do `.claude/agents`
  passou a virar string entre aspas. O texto contem "regressao: roda", e o
  dois-pontos sem aspas quebrava o parse do YAML inteiro — o agente nao
  carregava em sessao nova nenhuma, e a equipe rodava com 6 dos 7 papeis,
  sem o Tester (#1107).
