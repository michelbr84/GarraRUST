- **`garra whatsapp` conecta o WhatsApp pessoal lendo um QR code no terminal
  (#1238, ADR 0023).** Um comando, um menu de duas opcoes: numero pessoal por
  QR (`garra whatsapp link`) ou WhatsApp Business pela Cloud API oficial da
  Meta (`garra whatsapp cloud`). Tambem `garra whatsapp status` e `garra
  whatsapp logout`. Sem terminal o comando nao trava nem falha: imprime as
  duas opcoes com o comando de cada uma e sai 0, como o `garra init`.
  A sessao fica cifrada em repouso (AES-256-GCM, a mesma pilha do
  `CredentialVault`) em `<data_dir>/whatsapp/default/session.enc`, com
  arquivos 0600 dentro de um diretorio 0700 e escrita atomica. A chave e
  derivada de `GARRAIA_VAULT_PASSPHRASE` quando ela existe; sem ela fica num
  arquivo local e o `status` avisa disso em letras claras. `enabled = true` so
  e gravado na config **depois** de a sessao existir em disco, entao um
  pareamento abortado nao deixa o gateway pagando timeout a cada boot. Antes
  do primeiro QR ha uma tela de consentimento: cliente de aparelho vinculado
  nao e oficial, a conta pode ser bloqueada e a recomendacao e usar um numero
  secundario. **Nada destrutivo acontece antes dessa ultima confirmacao**: quem
  pede para re-vincular so tem a sessao atual posta de lado depois de aceitar o
  consentimento e de o Node ser encontrado, entao recusar, dar Ctrl+C ou nao
  ter Node deixa o vinculo que funcionava exatamente como estava. E `garra
  whatsapp cloud` rodado so para trocar um token preserva as demais chaves que
  o operador ja tinha em `channels.whatsapp`. Vincular precisa de Node.js 20 ou
  mais novo — so este caminho precisa; a Cloud API nao.
  **O pareamento de ponta a ponta com um telefone real e validacao manual**, e
  esta descrito passo a passo em `docs/whatsapp.md`. O CI cobre protocolo,
  maquina de estados, store cifrado, desenho do QR e ciclo de vida do processo
  filho contra a ponte falsa em Python, sem Node e sem telefone.
  **A ligacao com o gateway (canal pull `whatsapp_linked` e o check em
  `/api/diagnostics`) chega no slice seguinte**: por ora o comando vincula e
  guarda a sessao, e o gateway ainda nao le esse canal.
