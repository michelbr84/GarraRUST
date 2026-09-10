- O admin passa a ter troca de senha do proprio usuario, pela UI e por rota:
  `POST /admin/api/change-password` recebe a senha atual e a senha nova,
  reverifica a atual com o mesmo `verify_password` da danger zone antes de
  mexer em qualquer coisa e so entao grava o novo hash. Antes disso a unica
  forma de trocar a senha era SQL manual no `admin.db` (#1120).
- A pagina Account do console traz o formulario: senha atual, senha nova e
  confirmacao, com validacao de tamanho minimo de 8 caracteres e de igualdade
  entre os dois campos antes de sair o pedido. E a primeira pagina de conta
  acessivel a qualquer papel — ate hoje so havia telas de gestao de usuarios,
  que exigem papel admin (#1120).
- O hash continua sendo o PBKDF2-HMAC-SHA256 local do `admin/store.rs`, com
  os mesmos 600 mil iteracoes: nada muda para as linhas ja existentes e o
  Argon2id do `garraia-auth` segue fora do escopo do admin, que e SQLite e nao
  o provedor de identidade do workspace (#1120).
- Cada terminal escreve auditoria (`action` `change_password`, recurso `user`),
  com `outcome` `success` ou `failure`, e nenhuma delas carrega senha — nem a
  atual, nem a nova. A resposta de erro repete o formato `{"error": ...}` do
  resto da API do admin, e a falha de gravacao devolve uma mensagem fixa, com o
  detalhe do SQLite indo so para o log (#1120).
- As outras sessoes do usuario sao revogadas NA MESMA TRANSACAO do novo hash,
  pela `rotate_password_and_revoke_sessions`: ou as duas coisas acontecem,
  ou nenhuma — a rota nunca responde sucesso com o hash trocado e um cookie
  roubado ainda valido. A sessao que fez o pedido e mantida, entao o console
  nao se desloga no meio do proprio submit. Segue sem rate limit dedicado na
  rota, como todo o resto do router do admin (so o governor per-IP global);
  o custo de 600 mil iteracoes de PBKDF2 por tentativa errada e o freio que
  existe hoje (#1120).
