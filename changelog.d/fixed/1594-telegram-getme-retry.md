- **Telegram: falha de rede no GetMe deixa de ser reportada como token inválido, e ganha retry (#1594).** O `connect()` fazia um único `get_me()` e tratava QUALQUER erro como "Telegram token inválido" — sob Termux/Android uma falha de SSL/DNS (Bionic sem `SSL_CERT_FILE`) mandava o operador trocar um token que estava certo, e o canal nem subia.

Agora o erro é classificado: `Unauthorized`/`Not Found` (a API respondeu) continua "token inválido" e falha de primeira; rede/IO é transitória e re-tenta com backoff exponencial (3 tentativas) antes de falhar — e a mensagem final diz que NÃO é o token e aponta o diagnóstico de rede (Termux: `SSL_CERT_FILE`, bloco Termux do `garraia doctor`). Cada tentativa loga classe, contagem e o erro cru para o diagnóstico em ambiente restrito. A política de retry é genérica e testada sem rede: transitório recupera, token inválido não gasta retry, esgotado devolve a contagem certa.

Refs #1594
