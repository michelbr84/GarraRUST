- **#1133 a documentacao do 2FA para de prometer uma cifragem que nao existe.**
  Quatro lugares (docs/security.md, o modulo TOTP do mobile, o TOTP do painel
  admin e a doc da store admin) diziam que o segredo do segundo fator era
  guardado cifrado; na verdade ele fica em claro base32 no banco —
  `admin_users.totp_secret` no admin.db e `mobile_users.totp_secret` no
  SQLite do gateway, paridade entre os dois fluxos. A doc agora descreve o
  estado real: a mitigacao e proteger o arquivo de banco (a mesma que o
  token de sessao ja usa), o login do painel admin exige o codigo TOTP
  quando ativado (o /auth/login mobile nao exige), e cifrar o lado admin
  com a chave mestra do painel e a issue #1141.
