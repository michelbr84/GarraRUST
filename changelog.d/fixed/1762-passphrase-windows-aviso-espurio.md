- **`config check` no Windows deixa de acusar grafia deprecated da passphrase quando so a canonica esta definida (#1762).**
  Variaveis de ambiente sao case-insensitive no Windows, entao `GarraIA_VAULT_PASSPHRASE` e
  `GARRAIA_VAULT_PASSPHRASE` sao a mesma variavel: ler a grafia legada devolvia o valor da
  canonica e o check emitia o aviso espurio, que com `--strict` virava exit 2 numa config
  correta. No Windows os dois avisos de grafia (deprecated e valores divergentes) deixam de
  ser emitidos, porque as grafias nao sao distinguiveis la; em Unix nada muda. Teste
  `cfg(windows)` prova o caso.
