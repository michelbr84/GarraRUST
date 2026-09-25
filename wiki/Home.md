# GarraIA — Wiki

Bem-vindo à wiki pública do **GarraIA** — framework de agentes de IA em Rust, **100% local**, open source (MIT). Binário único de ~16 MB, ~13 MB de RAM em idle, com memória vetorial, modo de voz e 11 canais de chat (Telegram, Discord, Slack, WhatsApp, iMessage e mais) — seus dados nunca saem da sua máquina.

- **Repositório:** https://github.com/michelbr84/GarraRUST
- **Site:** https://garraia.org
- **Releases:** https://github.com/michelbr84/GarraRUST/releases

## Escolha seu caminho

| Quero… | Comece por |
|---|---|
| Instalar e rodar em 5 minutos | [Instalação e Primeiros Passos](Instalacao-e-Primeiros-Passos) |
| Conhecer os comandos do CLI | [Referência da CLI](Referencia-da-CLI) |
| Configurar provedores, canais e secrets | [Configuração](Configuracao) |
| Conectar Telegram, voz, MCP, VS Code, plugins | [Guias de Integração](Guias-de-Integracao) |
| Entender como funciona por dentro | [Arquitetura e ADRs](Arquitetura-e-ADRs) |
| Ligar dispositivos fisicos (MQTT, Home Assistant, Arduino, Raspberry Pi) | [docs/hardware.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware.md) |
| Operar com segurança / reportar vulnerabilidade | [Segurança e Operação](Seguranca-e-Operacao) |
| Contribuir ou ver o roadmap | [Contribuir, Roadmap e FAQ](Contribuir-Roadmap-e-FAQ) |

> Esta wiki é a **porta de entrada**: o conteúdo técnico canônico vive versionado em [`docs/`](https://github.com/michelbr84/GarraRUST/tree/main/docs) e no [README](https://github.com/michelbr84/GarraRUST/blob/main/README.md).

## Novidades

- **[v0.4.6 — Novidades](Novidades-v0.4.6)** · **[What's New in v0.4.6 (English)](Whats-New-v0.4.6)**
  — estabilização: CI sem registry para o MinIO, workspace padrão isolado por
  sessão em vez de `NoRoots`, `X-Session-Id` forjado sem acesso à conversa
  alheia, `garraia whatsapp users/remove/owner`, diagnóstico que separa "nunca
  configurado" de "quebrado", e o gate de dogfood no runbook de release.
- **[v0.4.5 — Novidades](Novidades-v0.4.5)** · **[What's New in v0.4.5 (English)](Whats-New-v0.4.5)**
  — `bash` só dentro de sandbox onde não há humano no laço, o "sim" a um
  pedido de confirmação aprovando em todo canal, `garraia whatsapp allow` e a
  ponte que se atualiza no boot, e um boot que recusa bind exposto sem
  credencial.
- **[v0.4.4 — Novidades](Novidades-v0.4.4)** · **[What's New in v0.4.4 (English)](Whats-New-v0.4.4)**
  — WhatsApp pessoal funcionando numa instalação nova, perfis de execução
  `standard` e `isolated-pod` (poder total dentro do pod, nada implícito
  fora), MCP `filesystem` sem `$HOME` e `tool_program` com gate por passo.
- **[v0.4.3 — Novidades](Novidades-v0.4.3)** · **[What's New in v0.4.3 (English)](Whats-New-v0.4.3)**
  — WhatsApp pessoal por dispositivo vinculado (`garra whatsapp` + QR),
  sandbox por tool `agent.sandbox`, cinco fail-opens do MCP fechados e um
  LLM padrão só.
- **[v0.4.2 — Novidades](Novidades-v0.4.2)** · **[What's New in v0.4.2 (English)](Whats-New-v0.4.2)**
  — o Garra saiu da tela: MQTT, Home Assistant, serial/USB e GPIO sob um
  modelo de risco R0-R5, streaming no app, e o canal do `/proc` fechado.
- **[v0.3.9 — Novidades](Novidades-v0.3.9)** · **[What's New in v0.3.9 (English)](Whats-New-v0.3.9)**
  — 48 issues e PRs: terminal que mostra o que o agente faz, memoria
  inspecionavel, e os modos de execucao passando a valer no executor.

## Changelogs

- [Semana 21 (18/05 – 24/05)](Changelog-Semana-21)
- [Semana 20 (11/05 – 17/05)](Changelog-Semana-20)
- [Semana 19 (04/05 – 10/05)](Changelog-Semana-19)

Histórico completo de versões: [CHANGELOG.md](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · [todas as releases](https://github.com/michelbr84/GarraRUST/releases).
