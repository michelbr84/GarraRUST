# ADR 0014 — Superfície Anthropic-compatible no gateway (`POST /v1/messages`)

**Status:** Accepted (2026-08-30, horário da Flórida)
**Contexto:** plan 0361, epic `garra agents setup`
**Supersedes:** nada. **Superseded by:** nada.

## Contexto

O comando `garra agents setup` configura quatro agentes com um mesmo par
provedor+modelo. Três deles (GarraIA, OpenClaw, Hermes) aceitam um provedor
OpenAI-compatible e um modelo arbitrário. O **Claude Code** não: ele fala
exclusivamente o wire da Anthropic Messages API, e tem um único campo de
modelo (`ANTHROPIC_MODEL`), sem slot de fallback.

Havia três caminhos:

1. Deixar o Claude Code fora da configuração unificada, na autenticação
   Anthropic nativa.
2. Apontar `ANTHROPIC_BASE_URL` direto para a OpenRouter.
3. Apontar `ANTHROPIC_BASE_URL` para o gateway do GarraIA, e o gateway traduz.

## Decisão

**Opção 3.** O gateway ganha `POST /v1/messages` e
`POST /v1/messages/count_tokens`, traduzindo o wire da Anthropic para o
`LlmRequest` interno e de volta.

Isso dá ao Claude Code duas coisas que ele não tem sozinho: qualquer provedor
que o GarraIA saiba falar, e **failover primário→backup**, porque o gateway já
implementa `complete_with_fallback` / `stream_complete_with_fallback` com
circuit breaker (GAR-210).

## Consequências

### Aceitas

- **A instalação do Claude Code deixa de usar a autenticação de assinatura
  Anthropic.** O uso passa a ser cobrado por token no provedor configurado. Para
  quem tem Pro/Max isso é uma troca de tarifa plana por consumo, e precisa estar
  dito no wizard, no painel e aqui. `agents setup` avisa; `--skip claude-code`
  e a página Providers deixam reverter.
- **Prompt caching não é repassado.** O `cache_control` que o Claude Code manda
  nos blocos de `system` é tolerado e descartado. Em conversa longa isso
  multiplica o custo de entrada.
- **O gateway vira dependência de runtime do Claude Code.** Com ele fora do ar,
  aquele Claude Code não responde.
- **Superfície local de proxy de credencial.** Um `/v1/messages` em 127.0.0.1
  que faz proxy de uma chave paga é escalada de privilégio para qualquer
  processo local. Mitigação: o gateway já tem camada de auth em `/v1/*`, e o
  bind é loopback por padrão. Antes de qualquer bind fora do loopback, exigir
  token — mesma regra que vale para as rotas A2A.

### Invariantes de implementação

1. **É proxy, não agente.** O caminho não passa por
   `AgentRuntime::process_message_*`, que injeta as tools do próprio GarraIA e
   as executa. O Claude Code precisa dos blocos `tool_use` crus de volta para
   rodar o próprio Read/Edit/Bash; passar pelo runtime significaria que ele
   nunca conseguiria editar arquivo nenhum.
2. **Sem hidratação de sessão.** A API da Anthropic é stateless e o cliente
   reenvia a conversa inteira a cada turno. Reusar o histórico do gateway
   duplicaria contexto.
3. **`stop_reason: "tool_use"` quando há bloco `tool_use`.** Decidido pela
   presença do bloco, não só pelo `finish_reason` do provedor — os modelos da
   OpenRouter são inconsistentes nesse campo. Errar aqui faz o Claude Code
   *imprimir* as tools em vez de executá-las.
4. **Eventos SSE nomeados.** O stream OpenAI-compatible do gateway usa
   `Event::default().data(...)` sem nome; clientes Anthropic despacham pelo
   nome do evento.
5. **`usage` nunca zero.** O `message_start` alimenta o medidor de contexto e o
   gatilho de auto-compact do cliente. Zero reportado é tratado como "não sei" e
   substituído por estimativa (`chars/4`); repassar zero faria o transcript
   crescer sem limite até o provedor recusar por contexto.
6. **Streaming sintetizado quando o provedor não streama.** O `OllamaProvider`
   não implementa `stream_complete` no trait — e Ollama é justamente o backup
   padrão. O envelope SSE é montado sobre `complete()` nesse caso.
7. **Nunca registrar `GET /v1/models` aqui.** `build_openai_router` já registra
   essa rota e o Axum entra em pânico no boot com método+path duplicados.

## Alternativas rejeitadas

- **(1) Deixar o Claude Code de fora** — preserva assinatura e caching, mas
  contradiz o pedido de "configurar todos simultaneamente" e deixa o agente mais
  usado fora do painel.
- **(2) Apontar direto para a OpenRouter** — funciona, mas dá ao Claude Code
  zero failover (campo de modelo único) e coloca a chave da OpenRouter dentro de
  `~/.claude/settings.json`, somando mais uma cópia do segredo.
