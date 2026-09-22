# Guias de Integração

Índice curado dos guias versionados no repo, por eixo.

## Canais de chat

- [Visão geral dos 11 canais](https://github.com/michelbr84/GarraRUST/blob/main/docs/channels.md)
- [Conectar bot do Telegram (passo a passo)](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/guides/connect-telegram.md)
- [iMessage (macOS)](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/channels/imessage.md)
- [WhatsApp pessoal (dispositivo vinculado, QR): `garra whatsapp`](https://github.com/michelbr84/GarraRUST/blob/main/docs/whatsapp.md) — precisa de Node.js 20+ e npm só nesse caminho; a Cloud API da Meta segue em [docs/channels.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/channels.md)
  - **Quem entra e o que pode fazer**: `channels.whatsapp_linked.allow` decide quem é admitido (piso `default_mode`, default `search`, só leitura); `owners` (v0.4.4, ADR 0024) declara o **dono** — mesma normalização de `allow` (dígitos ou JID `@lid`) — que, **só** com `execution.profile: isolated-pod` e **só** em conversa 1:1, recebe o piso `code` (`bash`, `file_write`, tools MCP). Pareamento por código nunca confere isso; grupo nunca herda; `owners` em `standard` é só um `Warning` no `config check`. Detalhes: [docs/whatsapp.md § Dono e perfil de execução](https://github.com/michelbr84/GarraRUST/blob/main/docs/whatsapp.md) · [docs/execution-profiles.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/execution-profiles.md)
  - **Autorizar depois do QR (v0.4.5, #1345)**: o `garraia whatsapp link` pergunta o número que pode falar com o GarraIA (com código do país) e só diz "pronto" quando há alguém autorizado; sem terminal, `garraia whatsapp allow <número> [--owner] [--yes]` acrescenta a `allow` (ou `owners`, só em `isolated-pod`). O gateway relê `allow`/`owners` a cada mensagem — autorizar e revogar (apagando do `config.yml`) valem sem reiniciar quando ele subiu com o canal ligado e com o `config.yml` no disco. Contato que o WhatsApp identifica só por `@lid`, sem número, não casa com um número do `allow`: o `status` avisa, e a saída é um código `/pair` ou `garraia whatsapp allow <id>@lid`. Com ninguém autorizado toda mensagem é ignorada em silêncio: `garraia whatsapp status` mostra `Autorizados: 0`. Detalhes: [docs/whatsapp.md § Quem pode falar com o GarraIA](https://github.com/michelbr84/GarraRUST/blob/main/docs/whatsapp.md)

## Provedores LLM

- [Os 15 provedores e como configurar](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/providers.md)
- [LM Studio / Ollama (modelos locais)](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/guides/add-lm-studio.md)

## Voz (STT/TTS)

- [Voice Mode: Whisper + Chatterbox/Hibiki, pipeline completo](https://github.com/michelbr84/GarraRUST/blob/main/docs/voice.md) — ativar com `garra start --with-voice`

## Dispositivos físicos (hardware)

- [A plataforma de hardware](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware.md) — as camadas, o modelo de risco R0-R5 e o motor de automações
- [Hardware skills: adapters e presets empacotados](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware-skills.md) — o formato do manifesto e as duas regras que um skill não escolhe
- [Skills oficiais](https://github.com/michelbr84/GarraRUST/tree/main/skills/hardware) — Home Assistant, MQTT, serial/Arduino, ESP32, Zigbee e Matter

> Zigbee e Matter chegam como **preset sobre o Home Assistant**: a stack vive
> no hub, e o Garra herda as entidades. Nada liga sozinho — sem seção no
> config, o registro de dispositivos nasce vazio.

## MCP (Model Context Protocol)

- [Configurar servidores MCP (stdio + HTTP)](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/mcp.md) (versão mais completa; há um resumo em [docs/mcp.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/mcp.md))
- [GarraIA como servidor MCP: `garra mcp-server`](https://github.com/michelbr84/GarraRUST/blob/main/docs/cli-mcp-server.md)

## IDE / VS Code

- [Gateway OpenAI-compatible no VS Code](https://github.com/michelbr84/GarraRUST/blob/main/docs/vscode/setup.md)
- [Templates para Continue.dev](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/continue-modes.md)

## Plugins WASM

- [Visão geral do sistema de plugins](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/plugins.md)
- [Tutorial: criar um plugin (Rust → WASM)](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/guides/create-plugin.md)
- [Referência do Plugin SDK](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/guides/plugin-sdk.md)

## Ferramentas do agente

- [Tools embutidas](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/tools.md) · [Modos de execução](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/modes.md) · [Sistema de memória](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/memory.md)
- [Sandbox por tool (`agent.sandbox`)](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md) — §5.13 do threat model: o que cada backend (`docker`/`podman`/`ssh`) garante e não garante; config de referência em [`config.hardened.example.yml`](https://github.com/michelbr84/GarraRUST/blob/main/config.hardened.example.yml)
