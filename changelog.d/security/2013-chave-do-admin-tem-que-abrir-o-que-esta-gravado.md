- **A chave mestra do admin tem que abrir o que esta gravado (#2013).** Com
  segredos do admin gravados (chaves de provider, versoes arquivadas, segredos
  do 2FA), o boot passa a exigir que a chave resolvida abra **ao menos um**
  deles. Antes, uma frase-senha (`GARRAIA_ADMIN_KEY` /
  `GARRAIA_VAULT_PASSPHRASE`) trocada depois do re-key, ou um `master.key`
  trocado por outro, derivava uma chave que nao abria nada: 500 em toda
  leitura, 500 no login de quem tem 2FA, e os segredos novos cifrados sob a
  chave errada. Agora o painel fica em 503 `{"error":"admin_unavailable"}`,
  como nos outros casos do #1992, com uma linha `ERROR` que aponta a causa
  provavel e a saida. A regra e "nenhum abre", e nao "algum nao abre": uma
  linha corrompida sozinha nao derruba o painel. Sem frase-senha, um
  `master.key` ausente, ilegivel ou de tamanho errado deixa de ser trocado por
  uma chave aleatoria nova quando ha segredos gravados (o arquivo fica
  intocado e o painel em 503); sem nenhum segredo gravado a regeneracao
  continua, porque nao ha o que perder. Instalacao sem ciphertext sobe como
  antes. Ver `docs/security.md`, "Admin console answering 503".
