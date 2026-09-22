- **`garra whatsapp users` lista quem pode falar com o GarraIA (#1393).** O `status`
  dizia so QUANTOS estavam autorizados, e quem tinha liberado tres numeros meses
  atras precisava abrir o `config.yml` a mao para saber quais eram. O comando novo
  mostra o papel (`allow` ou `owners`) e os quatro ultimos digitos de cada
  identidade — nunca o numero inteiro, nem na tela nem no `--json`, que sai como
  documento unico (`{enabled, authorized, owners, users[]}`) para script.
