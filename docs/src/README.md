# GarraIA

O framework de agentes de IA open-source, seguro e leve.

Um único executável de **17 MB** que executa seus agentes de IA no Telegram, Discord, Slack, WhatsApp e iMessage — com armazenamento de credenciais criptografado, recarregamento dinâmico de configuração e apenas **13 MB de RAM em idle**. Construído em **Rust** para oferecer o nível de segurança e confiabilidade que agentes de IA exigem.

---

# Por que GarraIA?

Comparação com OpenClaw, ZeroClaw e outros frameworks de agentes de IA:

| Recurso                        | GarraIA                         | OpenClaw (Node.js)         | ZeroClaw (Rust)       |
| ------------------------------ | ------------------------------- | -------------------------- | --------------------- |
| Tamanho do executável          | 17 MB                           | ~1.2 GB (com node_modules) | ~25 MB                |
| Memória em idle                | 13 MB                           | ~388 MB                    | ~20 MB                |
| Inicialização (cold start)     | 3 ms                            | 13.9 s                     | ~50 ms                |
| Armazenamento de credenciais   | Vault criptografado AES-256-GCM | Arquivo plaintext          | Arquivo plaintext     |
| Autenticação padrão            | Ativada (pareamento WebSocket)  | Desativada por padrão      | Desativada por padrão |
| Agendamento                    | Cron, intervalo, execução única | Sim                        | Não                   |
| Roteamento multi-agente        | Planejado (#108)                | Sim (agentId)              | Não                   |
| Orquestração de sessões        | Planejado (#108)                | Sim                        | Não                   |
| Suporte a MCP                  | Stdio                           | Stdio + HTTP               | Stdio                 |
| Canais suportados              | 5                               | 6+                         | 4                     |
| Provedores LLM                 | 14                              | 10+                        | 22+                   |
| Binários pré-compilados        | Sim                             | Não aplicável (Node.js)    | Compilar manualmente  |
| Recarregamento de configuração | Sim                             | Não                        | Não                   |
| Sistema de plugins WASM        | Sim (sandbox seguro)            | Não                        | Não                   |

Benchmarks medidos em uma instância DigitalOcean com:

* 1 vCPU
* 1 GB de RAM

Você pode reproduzir esses resultados por conta própria.

---

# Recursos

## Provedores LLM

15 provedores suportados:

* Anthropic Claude
* OpenAI
* OpenRouter
* Ollama
* 11 provedores compatíveis com OpenAI:

  * Sansa
  * DeepSeek
  * Mistral
  * Gemini
  * Falcon
  * Jais
  * Qwen
  * Yi
  * Cohere
  * MiniMax
  * Moonshot

Observação: o OpenRouter permite rotear para vários modelos/provedores através de uma única API, mantendo compatibilidade com o formato de chat completions.

---

## Canais de comunicação

Suporte completo para:

* Telegram
* Discord
* Slack
* WhatsApp
* iMessage

---

## MCP (Model Context Protocol)

Permite conectar qualquer servidor compatível com MCP para expandir as capacidades do agente com ferramentas externas.

---

## Runtime do agente

Inclui:

* 6 ferramentas integradas:

  * bash
  * file_read
  * file_write
  * web_fetch
  * web_search
  * schedule_heartbeat

* Memória persistente com busca vetorial

* Execução de tarefas agendadas

* Execução multi-etapas

---

## Skills

Permite definir habilidades do agente usando arquivos Markdown.

---

## Infraestrutura

Inclui:

* Recarregamento automático de configuração
* Execução como daemon
* Atualização automática
* Ferramentas de migração

---

# Estrutura da documentação

## Getting Started

Guia inicial para instalação e configuração do GarraIA.

## Architecture

Descrição da arquitetura interna.

## Channels

Configuração dos canais de comunicação.

## Providers

Configuração dos provedores de LLM.

## Tools

Referência das ferramentas integradas.

## MCP

Integração com ferramentas externas via Model Context Protocol.

## Security

Recursos e arquitetura de segurança.

## Plugins

Extensão do GarraIA usando plugins WebAssembly.