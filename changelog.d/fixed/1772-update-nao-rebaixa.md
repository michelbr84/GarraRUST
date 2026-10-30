- **`garra update` nao convida mais a rebaixar (#1772).** O #1320 corrigiu o convite
  de rebaixar no aviso, mas o `update` em si comparava as versoes por igualdade:
  um binario a frente do canal (`0.5.0` contra `latest` `0.4.10` — o caso do
  rc1) recebia `Update to v0.4.10? [Y/n]` com default(true). O `run_update` agora
  usa o mesmo gate `release_is_newer`: so `latest > current` oferece; binario mais
  novo que o canal imprime a mensagem e sai 0 sem prompt.
