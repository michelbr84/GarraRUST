- **`garraia whatsapp allow`, `remove`, `owner` e `unowner` passam a auditar
  (#1414; ADR 0025).** Os comandos legados gravavam `allow`/`owners` no
  `config.yml` sem deixar rastro, e a trilha de `access audit` so contava as
  mutacoes novas (`level`, `write`, `block`, `open`...). Agora cada escrita
  que de fato mudou algo vai para o mesmo `<data_dir>/audit/whatsapp-access.jsonl`,
  com o nome do subcomando como acao (`allow`, `remove`, `owner`, `unowner`;
  `allow --owner` e `allow`, e e o resumo `depois` que diz que o alvo entrou
  como dono), origem `cli`, o usuario do SO como ator e o alvo mascarado
  (`...1234`) — nunca a identidade inteira, chave ou mensagem. Repeticao
  idempotente (autorizar quem ja estava, remover quem nao estava) nao gera
  evento. O passo pos-QR do `link`, que e a mesma escrita do `allow`, audita
  do mesmo jeito. Mudanca gravada com o audit indisponivel sai 73, como nos
  comandos novos; no `link` o exit continua 0 (o vinculo valeu) e o aviso sai
  em stderr.
