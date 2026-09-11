- **O segredo TOTP do painel admin passa a ser cifrado no `admin.db`
  (#1141).** Ele ficava em claro (base32) na coluna `admin_users.totp_secret`
  e agora e gravado em `totp_secret_enc`/`totp_secret_nonce` com AES-256-GCM
  sob a chave mestra do painel — a mesma que `admin/secrets.rs` ja usa para
  as chaves de provider no mesmo arquivo. Isso tira o comprometimento
  **duravel** do 2FA: o segredo em claro sobrevivia a expiracao da sessao e a
  troca de senha. Banco existente migra sozinho na primeira leitura (lazy
  upgrade forward-only, que zera a coluna antiga) — nao ha passo de operador.
  Decifrar falhando e indisponibilidade, nao "2FA desligado": trocar a chave
  mestra sem re-cifrar recusa o login em vez de abrir o painel so com a
  senha.
- **O re-key da chave mestra do painel passa a levar o segredo do 2FA junto
  (#1141).** Cifrar o segredo criou uma dependencia da chave mestra que a
  rotacao de parametros KDF nao conhecia: ela re-cifrava `secrets` e
  `secret_versions` e deixava `admin_users.totp_secret_enc` sob a chave
  antiga, o que virava HTTP 500 permanente no login de todo admin com 2FA
  ligado. As colunas do 2FA entram na mesma transacao do re-key, e um
  segredo que nao decifra com a chave legada aborta o re-key inteiro em vez
  de gravar parametros que nao abrem nada. Um `master.key` ilegivel tambem
  deixou de ser substituido em silencio: ele e preservado como
  `master.key.unreadable` com aviso no log.
