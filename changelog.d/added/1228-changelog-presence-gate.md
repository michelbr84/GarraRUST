- **PR sem fragmento em `changelog.d/` fica vermelho (#1228).** O job de
  fragmentos do CI validava so o formato do que existia; um PR que nao
  escrevia fragmento nenhum passava, e como as notas de release saem do
  CHANGELOG.md o silencio so aparecia no dia da release. O novo workflow
  `Changelog presence` exige que o PR adicione ou edite um
  `changelog.d/<secao>/<nome>.md` de secao valida. Isentos: a label
  `no-changelog` (aplicar ou tirar reexecuta o check), o dependabot, branches
  `release/*` e eventos fora de `pull_request`. Roda em `pull_request` com
  `contents: read`, sem segredo, sem ler titulo nem corpo do PR, e le os
  arquivos do proprio merge commit.
