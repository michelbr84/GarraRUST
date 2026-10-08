- **`garraia doctor mcp` valida que o MCP server autoprovisionado subiu de fato (#1595).** O doctor confere instalação, mas nunca o plano MCP: uma falha de subida (cache npx corrompido, binário ausente, handshake morto) passava despercebida — o doctor reportava ok enquanto o filesystem estava morto e o agente ficava sem tools.

O novo `doctor mcp` spawna cada servidor stdio declarado pelo MESMO código do boot do gateway (`build_mcp_tools`: mesmo merge de `config.yml`/`mcp.json`, mesma resolução de `vault:`, mesma recuperação de cache npx), conta as tools do handshake e desliga os filhos antes de sair. Exit 0 quando tudo subiu, **exit 2 quando algum servidor não subiu** (útil em CI/scripts), 65 para config que não parseia. Servidor `enabled: false` é decisão do operador e não reprova; entrada HTTP fica marcada como não-verificada por este check.

O doctor raiz ganha um resumo CONFIG do plano MCP (`[5/5]`, puro, sem spawn): quantos servidores declarados e se o `filesystem` está com a versão fixada (#1346) — o health-check de subida continua sendo a área explícita, para o onboarding rápido não virar download de npx.

Refs #1595
