# GarraIA — Insumos de custo para decisão de precificação — DADOS DE ENGENHARIA

> **Escopo estrito:** fornecer **dados de custo** para a decisão de negócio de
> precificação (issue de GA de negócio). **Não há decisão de preço aqui** —
> preço, modelo de receita e alvo são do maintainer.

## 1. Arquitetura de custo: BYOK (traga sua chave)

O produto é self-host e o roteamento de LLM é configurado pelo operador com a
**própria chave** (`OPENROUTER_API_KEY`, `ANTHROPIC_API_KEY`, OpenAI, Ollama
local etc.). Consequência: **custo de LLM é do usuário final, não do
projeto.** Em instalação self-host, a margem do projeto sobre tokens é zero
por construção.

## 2. Custo marginal do projeto por usuário self-host

| Item | Custo para o projeto | Observação |
|---|---|---|
| Distribuição do binário | ~0 | Release por tag; artefatos no GitHub (repositório público) |
| LLM do usuário | 0 (BYOK) | Usuário paga seu provedor |
| TTS/STT | 0 (BYOK ou local) | Chatterbox/Kokoro self-host; ElevenLabs com chave do usuário |
| CI | ~0 | Público no GitHub Actions |
| Canais (Telegram/Discord/etc.) | 0 | Plataformas gratuitas; WhatsApp exige API própria do operador |

**Custo marginal por usuário self-host hoje: praticamente zero para o
projeto.** Não há infra operada pelo projeto que escale com usuários.

## 3. Custo único identificável (canal desktop)

- **Certificado de assinatura de código EV/OV**: na ordem de **~USD 200–400/ano**
  (varia por autoridade — DigiCert/Sectigo/GlobalSign), requisito do canal
  distribuidor desktop (issue de GA de desktop). É o único custo recorrente
  real identificado até hoje para o projeto.

## 4. Custo de operação futura (só se existir oferta cloud)

Se o projeto operar cloud, o custo passa a escalar com uso: GPU/CPU para
gateway + (opcional) TTS/STT hospedados + banco + largura de banda de
mensagens. **Hoje não existe cloud operada** — qualquer número aqui seria
especulação; modelar exige a decisão de alvo/modelo primeiro.

## 5. Entradas que faltam para a decisão de negócio (não são de engenharia)

- Custo de aquisição de cliente (comunidade vs. venda enterprise).
- Valor de referência de mercado para agentes self-host.
- Custo de suporte humano por cliente pagante.

Refs: issue de negócio de GA (modelo de receita/precificação), issue de
desktop (certificado), `docs/operations/sla-decision-brief.md`.
