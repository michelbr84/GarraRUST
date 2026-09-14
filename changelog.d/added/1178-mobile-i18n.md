- **Garra Mobile em portugues do Brasil e ingles, com seletor de idioma (#1178).**
  O app era uma mistura de ingles e portugues sem opcao de idioma. Agora toda a
  copy de UI (telas, botoes, dialogos, erros, estados vazios, tooltips, canais de
  notificacao, prompt biometrico) sai de `lib/l10n/app_en.arb` + `app_pt.arb`
  via `context.l10n` (Flutter gen_l10n, 255 chaves, plurais e placeholders em
  ICU), e as strings do proprio Material (menu de selecao de texto, tooltips
  padrao) seguem o idioma via `flutter_localizations`. Settings ganha o cartao
  **Idioma**: Padrao do sistema / English / Portugues (Brasil), persistido em
  SharedPreferences e aplicado na hora, sem reiniciar; sistema em pt-BR abre o
  app em pt-BR (qualquer variante `pt` cai em pt-BR, o resto em ingles). Um teste
  (`test/l10n_hardcoded_strings_test.dart`) varre `lib/` e falha em string de UI
  hard-coded nova. Fica de fora, de proposito: valores que vem do runtime
  (nomes de provider/modelo, `status` de health, descricoes de comandos/modos
  do servidor) e o fallback `unknown` dos modelos — sao dados do gateway, nao
  copy do app.
