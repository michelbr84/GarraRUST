# Arquitetura

O GarraIA é um framework de agente de inteligência artificial desenvolvido em Rust, projetado com foco em desempenho, segurança e extensibilidade.

---

## Estrutura

```
crates/
  garraia-cli/        # Interface de linha de comando (CLI), assistente de inicialização e gerenciamento do daemon
  garraia-gateway/    # Gateway WebSocket, API HTTP e gerenciamento de sessões
  garraia-config/     # Carregamento de configurações YAML/TOML, recarregamento dinâmico e configuração do MCP
  garraia-channels/   # Integrações com Discord, Telegram, Slack, WhatsApp e iMessage
  garraia-agents/     # Provedores de LLM, ferramentas, cliente MCP e runtime do agente
  garraia-db/         # Memória baseada em SQLite e busca vetorial (sqlite-vec)
  garraia-plugins/    # Sandbox de plugins WASM (wasmtime)
  garraia-media/      # Processamento de mídia
  garraia-security/   # Cofre de credenciais, listas de permissão, pareamento e validação
  garraia-skills/     # Interpretador, scanner e instalador de arquivos SKILL.md
  garraia-common/     # Tipos compartilhados, erros e utilitários
```

---

## Ferramentas

O runtime do agente inclui 6 ferramentas nativas que o modelo de linguagem (LLM) pode invocar durante uma conversa. O loop de execução de ferramentas pode ocorrer por múltiplas iterações dentro de uma única mensagem, respeitando os limites de segurança do ExecutionBudget.

| Ferramenta           | Descrição                                                                                             |
| -------------------- | ----------------------------------------------------------------------------------------------------- |
| `bash`               | Executa comandos do sistema (timeout de 30 segundos, saída máxima de 32 KB)                           |
| `file_read`          | Lê o conteúdo de arquivos (máximo de 1 MB, com proteção contra acesso fora do diretório permitido)    |
| `file_write`         | Escreve conteúdo em arquivos (máximo de 1 MB, com proteção contra acesso fora do diretório permitido) |
| `web_fetch`          | Obtém páginas da web (timeout de 30 segundos, resposta máxima de 1 MB)                                |
| `web_search`         | Realiza buscas usando a API Brave Search (requer `BRAVE_API_KEY`)                                     |
| `schedule_heartbeat` | Agenda a reativação futura do agente (máximo de 30 dias, limite de 5 agendamentos pendentes)          |

Consulte [Ferramentas](./tools.md) para a referência completa.

---

## MCP (Model Context Protocol)

O GarraIA pode se conectar a servidores MCP externos para expandir suas capacidades. As ferramentas MCP são descobertas automaticamente na inicialização e aparecem como ferramentas nativas do agente, utilizando nomes com namespace (`servidor.nome_da_ferramenta`).

A configuração está localizada em:

```text
config.yml
```

na seção:

```text
mcp:
```

ou no arquivo:

```text
~/.garraia/mcp.json
```

(formato compatível com Claude Desktop).

Ambas as fontes são carregadas e combinadas automaticamente na inicialização.

O crate `garraia-agents` contém o cliente MCP (utilizando o crate `rmcp`), incluindo uma camada de adaptação que converte ferramentas MCP para o formato interno de ferramentas do GarraIA.

Consulte [MCP](./mcp.md) para a referência completa.

---

## Registros de Decisões Arquiteturais

Consulte:

```text
./adr/README.md
```

para acessar os registros completos de decisões arquiteturais do projeto.