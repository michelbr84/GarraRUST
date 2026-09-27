- **Plano da chave de sessao e modos apertados no link do WhatsApp (#1431).**
  `SessionKey::plan` diz o que o resolve faria no diretorio da conta (passphrase
  do cofre, `session.key` existente ou nova) sem criar arquivo nem derivar PBKDF2
  — o mesmo predicado (`usable_passphrase`, passphrase vazia conta como ausente)
  governa plano e resolve, e a configuracao do wizard passa a poder mostrar o
  caminho seguro antes de criar qualquer coisa. `SessionStore::harden_modes`
  aperta o diretorio da conta para 0700 e todo arquivo regular dentro dele para
  0600 (incluido o `recusas-lid.json` que o gateway grava) e devolve o que
  estava mais aberto, para a CLI avisar sem falhar; o salt passa a ser apertado
  ao ser lido, como a `session.key` ja era. Symlink dentro do diretorio nao e
  seguido nem reportado, modo mais fechado que o exigido fica como esta e fora
  de Unix a lista volta vazia.
