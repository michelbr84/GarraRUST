- **Todas as crates do workspace passam a declarar a MSRV (#1228).** Apenas 3
  das 23 crates herdavam `rust-version` do `[workspace.package]`; nas outras 20
  o cargo nao tinha como recusar um toolchain velho, e o erro so aparecia no
  meio da compilacao, atribuido a uma crate qualquer. Agora `garraia-glob`,
  `garraia-runtime` e `garraia-tools` trazem `rust-version = "1.95"` literal
  (elas ainda nao herdam nada do workspace: tem `version` e `edition`
  proprios, e migrar a heranca inteira mudaria os dois junto) e as demais usam
  `rust-version.workspace = true`. Nao ha `rust-toolchain.toml` de proposito:
  o CI roda lint e teste em `stable` e so o job de MSRV roda `cargo check` no
  piso, entao pinar 1.95 na arvore deixaria o `cargo clippy` local vermelho
  numa `main` verde.
- **O formato dos fragmentos de `changelog.d/` virou gate de CI (#1228).** O
  `assemble.py --check` existia desde sempre e so rodava se alguem lembrasse;
  uma auditoria achou 4 de 6 PRs abertas sem fragmento nenhum. Como as notas
  de release saem do `CHANGELOG.md` e nao da lista de commits, o silencio so
  aparecia no dia da release. O job novo tambem roda os testes do proprio
  `assemble.py` e dos parsers do quality ratchet — em invocacoes separadas,
  porque as duas suites trazem um pacote `tests` homonimo e uma chamada unica
  quebra na coleta. A checagem de *presenca* de fragmento fica para uma fatia
  propria, que precisa de label de escape e de periodo em observacao.
- **O roster no `.claude/agents/team-coordinator.md` estava defasado (#1228).**
  A tabela listava deepseek, glm e gpt e afirmava que a independencia de
  julgamento vinha de o Reviewer usar um modelo diferente do Implementer. O
  roster e Claude desde 2026-09-14 e a independencia vem do contexto separado
  (agente, prompt e worktree distintos, com o Reviewer lendo o diff e nunca o
  relatorio do Implementer) — como `skills/assemble-team.md` e o `CLAUDE.md` ja
  diziam. As duas skills de orquestracao ganharam uma secao de pre-requisitos
  dizendo que sem ferramenta de spawn de subagente o R4 nao e mergeavel por
  construcao, e o que fazer nesse caso.
- **Quem muta a arvore trabalha em worktree propria (#1228).** `code-reviewer` e
  `test-engineer` passam a dizer isso explicitamente. Uma mutacao aplicada e
  restaurada numa worktree compartilhada apareceu, para o Implementer que
  trabalhava nela, como edicao nao-commitada desativando a maquina de estados:
  ele parou, reverteu para a versao auditada e reportou — comportamento certo,
  arranjo errado. Junto, `assemble-team.md` ganha o gate que faltava em R4: o
  `security-auditor` pede o controle e o `code-reviewer` **muta o controle**,
  provando que sua remocao fica vermelha. O caso que motivou: uma auditoria R4
  exigiu `env_clear()` no spawn do filho e a allowlist de ambiente, os dois
  foram aplicados, e apagar o `env_clear()` depois deixava 93 de 93 testes
  verdes — o teste afirmava o conteudo de duas constantes, nunca que o
  ambiente era limpo.

