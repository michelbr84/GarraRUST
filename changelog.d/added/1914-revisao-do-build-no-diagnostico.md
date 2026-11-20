- **`garra_diagnostics` diz de qual commit e tag saiu o binario (#1914).**
  `versao.instalada` ganha o bloco `build`: num build do GitHub Actions,
  `origem: "ci"` com a `revisao` (40 hex do `GITHUB_SHA`) e a `tag` quando o
  build rodou num push de tag (o caso da release, ex. `v0.5.2-rc5`); fora do
  Actions, `origem: "local"` e `estado: "nao_disponivel"`, sem revisao
  inventada. E o que separa o rc5 de um build posterior com a mesma versao
  `0.5.2`. Capturado em tempo de compilacao (nada de `git` no build.rs, nada
  de workflow alterado); revisao malformada e descartada junto com a tag, e
  o nome da tag passa por uma lista fechada de caracteres.
