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
