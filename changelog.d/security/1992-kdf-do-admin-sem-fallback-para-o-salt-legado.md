- **Chave mestra do admin nao recua mais para o salt constante legado (#1992).**
  Com `GARRAIA_ADMIN_KEY` ou `GARRAIA_VAULT_PASSPHRASE` definida, dois estados
  faziam `derive_with_passphrase` derivar do salt compartilhado de antes de
  2026-08-29 (100k iteracoes) a chave que CIFRA, com um `warn!` como unico
  sinal: linha de `kdf_params` ilegivel e falha ao gerar o salt por instalacao.
  No primeiro, a chave legada nem abria os segredos existentes, que ja estavam
  sob a chave nova. Os dois agora recusam o boot com mensagem que diz o motivo
  e a saida (restaurar o `admin.db` de um backup). A linha gravada passa a ser
  aceita so no formato que o gateway grava (versao conhecida, salt de 32 bytes,
  no minimo 600k iteracoes), o que fecha o downgrade de KDF por EDICAO da linha
  por quem escreve no `admin.db`; e o leitor `AdminStore::kdf_params` deixa de responder "sem
  linha" para linha ilegivel, que era um terceiro caminho ate o mesmo fallback.
  O re-key que apenas falha continua adiado para o proximo boot, na chave
  legada — o que inclui a linha APAGADA de uma instalacao ja re-chaveada, caso
  que esta correcao nao fecha e ficou na #1999. A chave passa a ser resolvida
  antes de os canais subirem, para a recusa nao deixar nada conectado. Instalacao sem passphrase (`master.key`) nao muda. Ver
  `docs/security.md`.
