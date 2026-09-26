- **Web Console: "Test WhatsApp" com o MESMO motor do `garraia doctor
  whatsapp` (#1420).** A tabela do doctor (fatos → linhas, agregado, exit
  code) saiu da CLI e mora em
  `garraia_gateway::bootstrap::whatsapp_linked_doctor`; a CLI virou
  consumidora e a saida dela (`--json`, tabela humana, exit 0/2/69) nao
  mudou. O gateway ganhou `GET /admin/api/whatsapp/doctor?lang=pt|en`
  (cookie do `/admin`, `Channels/Read` — viewer le), que colhe os fatos em
  processo — vinculo com a view REAL da ponte, chave da sessao pela mesma
  resolucao do boot (so quando ha sessao; nunca materializa arquivo numa
  rota de leitura), config viva, e as linhas do proprio `/api/diagnostics`
  por chamada de funcao, sem HTTP — e devolve `{ status, version, lang,
  checks }` com `checks` identico ao `report.checks` do `--json`. Na pagina
  WhatsApp Access, um card no topo roda o teste: resumo, uma linha por check
  (vinculo, chave, gateway e ponte, acesso, perfil de execucao, workspace,
  MCP visivel no piso, provider) e, para cada linha nao-verde, uma acao
  segura — rolar ate a matriz de acesso, abrir MCP Servers / Providers /
  Configuration, ou copiar o passo de terminal (mostrado, nunca executado).
  Funciona com o canal parcialmente configurado (e o caso do CI). Nenhum
  segredo, chave de sessao ou numero sai: teste de integracao alimenta chave
  de gateway e de provider e varre o corpo; spec Playwright varre o HTML do
  card.
