- **Chave mestra do admin nao recua mais para o salt constante legado
  (#1992, #1999).** Com `GARRAIA_ADMIN_KEY` ou `GARRAIA_VAULT_PASSPHRASE`
  definida, tres estados faziam o boot derivar do salt compartilhado de antes
  de 2026-08-29 (100k iteracoes) a chave que CIFRA, com um `warn!` como unico
  sinal: linha de `kdf_params` ilegivel, falha ao gerar o salt por instalacao
  e linha APAGADA depois do re-key. No primeiro e no terceiro, a chave legada
  nem abria os segredos existentes, que ja estavam sob a chave nova, e tudo o
  que o painel gravasse dali em diante se perdia quando a linha voltasse a ser
  lida. Agora nenhum deles entrega chave: o gateway sobe normalmente (chat,
  canais e `/api/*` seguem no ar), so o `/admin` (pagina e API) responde 503
  `{"error":"admin_unavailable"}`, e o motivo e a saida vao para o log numa
  linha `ERROR` que comeca com `admin panel disabled`. A linha gravada passa a
  ser aceita so no formato que o gateway grava (versao conhecida, salt de 32
  bytes, de 600k a 10M iteracoes), o que fecha o downgrade de KDF por EDICAO
  da linha; o leitor `AdminStore::kdf_params` deixa de responder "sem linha"
  para linha ilegivel; e o re-key so fica na chave legada quando todo segredo
  gravado abriu com ela e apenas a gravacao falhou (disco cheio, banco
  travado), caso em que o proximo boot tenta de novo. Uma frase-senha trocada
  numa instalacao ainda nao re-chaveada cai no mesmo 503. Instalacao sem
  passphrase (`master.key`) nao muda, exceto pela falta de entropia ao criar o
  arquivo, que deixa de derrubar o processo. Ver `docs/security.md`, "Admin
  console answering 503".
