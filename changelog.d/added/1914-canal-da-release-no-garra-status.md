- `garra_status` nao confunde RC com release estavel (#1914): o bloco
  `upstream` ganha `releases_all_api` (lista com os RCs) e `means`, que diz
  no proprio relatorio que `/releases/latest` so devolve a estavel mais nova
  e que a `version` nao distingue RC de estavel (os RCs saem com a versao
  pura do Cargo, `0.5.2` do rc1 ao final) nem build local de oficial. O
  relatorio nao deriva canal da versao; para identificar o build, aponta o
  SHA-256 do executavel em `garra_diagnostics` (`versao.executavel`) contra
  o `.sha256` da release candidata.
