- **Resposta de tarefa agendada chega ao Discord, Slack, Matrix e WhatsApp
  vinculado (#2002).** Antes so o Telegram recebia; nos outros canais a
  resposta (e o aviso de teto do #1985) ficava so no historico. O endereco do
  Discord, do Slack e do Matrix sai do id da sessao, e so quando o canal
  gravado na sessao concorda com o prefixo do id. No WhatsApp vinculado a
  entrega reconsulta a config viva (canal ligado, grupo com resposta ligada,
  quem criou a tarefa ainda admitido: contato bloqueado ou removido nao
  recebe), so sai com a ponte conectada, tem teto proprio de 3 por chat por
  minuto, separado da cadencia dos turnos, e leva a marca invisivel de
  auto-conversa. IRC e iMessage continuam sem entrega, de proposito (ver
  `docs/configuration.md`). O ledger de runs ganha a coluna `delivery`
  (`entregue`, `sem_entrega` ou `nao_entregue:<motivo>`), mostrada pelo
  `garraia runs list`, pelo `--json` e pelo `GET /api/runs`.
