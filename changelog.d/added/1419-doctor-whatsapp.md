- **`garraia doctor whatsapp` (#1419).** O caminho do WhatsApp pessoal de ponta
  a ponta numa passada: vinculo (`LinkHealth`, a mesma tabela do `status` e do
  console), chave da sessao (origem e prova de que abre), gateway e ponte
  (com o gateway de pe, o que o `/api/diagnostics` diz; sem ele, a linha diz
  que nao sabe), acesso (contagens de autorizados e donos), perfil de execucao
  (`isolated-pod` e sempre aviso, dizendo o que o dono ganha), raizes das file
  tools (o mesmo resolvedor do boot), MCP visivel no piso `search` (servidor
  inteiro, so operacoes de leitura, ou escondido — com a sintaxe para liberar)
  e provider padrao. Cada linha traz o proximo passo; `--json` fala o
  vocabulario do `/api/diagnostics` (`ok`/`warning`/`error`/`not_configured`);
  exit 0 tudo verde, 2 aviso sob `--strict`, 69 algo vermelho. Nenhuma linha
  carrega numero, LID, chave ou URL com credencial.
