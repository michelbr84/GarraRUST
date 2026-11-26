- **O jsDelivr deixa de ser espelho dos instaladores (#2023).** O jsDelivr
  recusa repositorio do GitHub acima de 50 MB, e o GarraRUST esta no limite:
  o `@main` alternava entre HTTP 403 (`Package size exceeded the configured
  limit of 50 MB`) e uma copia velha do cache. Sai da ajuda do `install.sh` e
  do `install.ps1`, das docs e das sondas do `install-endpoints.yml`; as
  suites `documented_urls` impedem que volte.
