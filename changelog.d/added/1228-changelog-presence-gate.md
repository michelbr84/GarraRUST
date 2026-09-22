- **PR sem fragmento em `changelog.d/` fica vermelho (#1228).** O job de
  fragmentos do CI validava so o formato do que existia; um PR que nao
  escrevia fragmento nenhum passava, e como as notas de release saem do
  CHANGELOG.md o silencio so aparecia no dia da release. O novo workflow
  `Changelog presence` exige que o PR adicione ou edite um
  `changelog.d/<secao>/<nome>.md` de secao valida. Isentos: a label
  `no-changelog` (aplicar ou tirar reexecuta o check), o dependabot, branches
  `release/vX.Y.Z` abertas deste repositorio (de fork o nome da branch nao
  isenta, porque quem escolhe e o autor) e eventos fora de `pull_request`.
  Roda em `pull_request` com `contents: read`, sem segredo, sem ler titulo
  nem corpo do PR, e le os arquivos do proprio merge commit com `git diff -z`,
  para que fragmento com acento no nome nao saia entre aspas e deixe de
  contar.
