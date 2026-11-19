- **O teste de Capabilities do console fixa a propria sessao `search` (#1951,
  refs #1915).** As assercoes de `denied_by` em `admin-sessions.spec.ts` so
  rodavam se a primeira linha da tabela estivesse em `search`; outra sessao
  criada antes as desligava e o teste seguia verde. Agora o teste semeia uma
  sessao propria, acha a linha pelo `data-session-id` e afirma sem condicao;
  no CI, falha se nao conseguir semear. So teste.
