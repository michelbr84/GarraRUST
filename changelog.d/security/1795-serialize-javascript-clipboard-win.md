- **`serialize-javascript` 7.1.4 no site de docs e `clipboard-win` 5.4.3 na CLI (#1795).**
  O override do `website/` sai da 7.0.5 e fecha o GHSA-m5mm-p8h7-m29v (XSS por
  `</script` no fim do corpo de funcao serializada; so no build do site). No
  Windows, a CLI deixa a `clipboard-win` 5.4.1, retirada do registro (yanked),
  cujo `get_html` podia ler fora dos limites com HTML de clipboard malformado.
  Ficam abertos, sem correcao aplicavel: `minimatch` 3.1.5 (o `serve-handler`
  fixa a versao, e a linha 3.x nao tem o codigo afetado) e `deepmerge` 4.3.1 (sem
  versao corrigida; so mescla a config estatica do SVGO no build).
