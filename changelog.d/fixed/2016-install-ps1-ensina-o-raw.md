- **A ajuda do `install.ps1` ensina o raw do `main`, nao o `garraia.org` (#2016).**
  A borda do hosting do site responde 403 com `cf-mitigated: challenge` ao
  cliente HTTP do PowerShell, entao `irm https://garraia.org/install.ps1 | iex`
  morria antes de baixar o script. Os exemplos e o `-Help` passam a usar
  `irm https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.ps1 | iex`,
  como o README e o site ja fazem. O `Content-Type` `application/octet-stream`
  nao era a causa: o `Invoke-RestMethod` decodifica qualquer resposta como
  texto. A sonda `install-endpoints.yml` deixa de asserir o `Content-Type` e
  usa o proprio `irm`: falha em qualquer URL que a documentacao ensina ao
  Windows, e so avisa no `garraia.org/install.ps1`, que saiu da documentacao.
