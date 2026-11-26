- **O one-liner do Windows baixa o `install.ps1` da ultima release (#2016).**
  `irm https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1 | iex`
  passa a ser o comando principal nas docs, na wiki e na ajuda do
  `install.ps1`: e o script que saiu com os binarios que ele instala, do
  mesmo `github.com` de que o instalador ja depende, sem passar pelo
  `raw.githubusercontent.com`, que a GitHub limita para trafego anonimo. O raw
  do `main` fica como espelho, para correcao que ainda nao saiu em release.
