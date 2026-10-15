# Introdução

**O framework seguro e leve de código aberto para agentes de IA** — escrito em
Rust, nascido no Brasil.

O GarraIA é um assistente de IA que roda na sua máquina. Um único binário nativo
executa seus agentes de IA em múltiplos canais (Telegram, Discord, Slack,
WhatsApp e iMessage), com cofre de credenciais opcional, recarregamento de
configuração a quente e um sistema completo de memória.

**Local-first** — todo o estado (conversas, memória, configuração, credenciais)
fica na sua máquina, sem telemetria. Seus prompts vão apenas para o provedor de
LLM que você configurar; com Ollama, o setup é 100% offline, sem egress.

> Esta documentação é **gerada a partir do markdown do próprio repositório**. Os
> arquivos-fonte continuam sendo a fonte única de verdade: um script de sync os
> copia para cá a cada build. O link "Edit this page" de cada página aponta para
> o arquivo real no GitHub.

## Por onde começar

- [Instalação](/instalacao/installation) — instale pelo instalador ou compile do código.
- [CLI e comandos](/cli/cli-ask) — `garra ask`, chat interativo, MCP server.
- [Gateway](/gateway/gateway-mcp-http) — servidor HTTP/WS, hardening e deploy.
- [MCP](/mcp) — Model Context Protocol no GarraIA.
- [WhatsApp](/whatsapp) — dispositivo vinculado e política de acesso.
- [Configuração](/configuracao/configuration) — o schema unificado de config.
- [Segurança](/seguranca/security) — auth, RLS, cofre, SSRF, modelo de ameaças.
- [Arquitetura](/arquitetura/architecture) — visão geral e os 25 ADRs.
- [Troubleshooting](/troubleshooting) — onde achar a solução de cada problema comum.
- [Changelog](/changelog) · [Releases](/releases) · [Roadmap](/roadmap)
- [Contribuição](/contribuicao/contributing) — como colaborar com o projeto.

## Idiomas

A maior parte da documentação está em **português (Brasil)**. Há uma versão em
**inglês** no seletor de idioma no topo da página. As páginas ainda não
traduzidas aparecem em português.
