# ADR 0012 — Persona Amistosa do Garra (Tom de Voz Padrão)

**Status:** Accepted
**Date:** 2026-06-01 (America/New_York)
**Epic:** GAR-771 — Garra Friendly Persona
**Plan:** [0250](../../plans/0250-gar-771-garra-friendly-persona.md)
**Inspiração:** [OpenHuman](https://github.com/tinyhumansai/openhuman) — "Simple, UI-first & Human"

---

## Context and Problem Statement

O Garra (GarraIA) é um gateway de IA pessoal multi-canal. Até o plano 0250, quando
o operador não definia `agent.system_prompt`, o runtime caía em
`unwrap_or_default()` → **system message vazio**. Na prática, o Garra herdava o tom
neutro do modelo base e não tinha identidade própria: parecia um LLM genérico, não
um "assistente pessoal chamado Garra".

O usuário pediu para o produto soar mais **amistoso**, no espírito do OpenHuman, que
se diferencia por ser caloroso, humano e de baixa fricção.

**Pergunta:** Como dar ao Garra uma voz própria e calorosa por padrão, sem remover o
controle do operador nem quebrar instalações existentes?

## Decision

1. **Persona padrão "Friendly"**: introduzir `garraia-agents::persona` com um system
   prompt caloroso em PT-BR (`DEFAULT_PERSONA_PT`) e equivalente EN
   (`DEFAULT_PERSONA_EN`). Quando nenhum `system_prompt` explícito está configurado,
   o runtime aplica essa persona.

2. **Override sempre vence**: qualquer `system_prompt` não-vazio (config ou override
   por requisição) tem prioridade absoluta sobre a persona padrão.

3. **Opt-out explícito**: `agent.persona = "neutral"` restaura exatamente o
   comportamento pré-0250 (sem system prompt padrão). `friendly` é o default.

4. **Idioma configurável**: `agent.persona_lang` ("pt-BR" default, "en" fallback).
   Tags desconhecidas caem em PT-BR (público primário).

5. **Copy de superfície humanizada**: `/start`, `/help`, banner CLI e wizard passam a
   falar com o tom do Garra (PT-BR, primeira pessoa, emoji parcimonioso).

6. **Erros humanizados**: estados de falha comuns para usuário novo (vault trancado
   sem `GARRAIA_VAULT_PASSPHRASE`, provider sem chave) ganham mensagens claras com o
   próximo passo, em vez de códigos crus.

## Tom de voz (resumo normativo)

- Primeira pessoa, natural, como um colega de confiança.
- Caloroso porém conciso; sem bajulação ("que ótima pergunta!").
- Honesto: se não sabe, diz e sugere um caminho.
- Emoji parcimonioso (👋 / 🐾), nunca poluir.
- PT-BR primeiro; EN como fallback paramétrico.
- Respeita usuário e privacidade.

## Consequences

**Positivas**
- Garra tem identidade desde o primeiro uso, sem configuração.
- Onboarding mais acolhedor (wizard/banner) reduz fricção.
- Erros deixam de intimidar usuários não-técnicos.
- Zero breaking change: `neutral` reproduz o comportamento antigo (test-provado).

**Custos / Riscos**
- A persona consome alguns tokens de system prompt por requisição (mitigável via
  `neutral` em cenários de custo extremo).
- Copy default em PT-BR pode não servir a todos; mitigado por `persona_lang`.

**Não-escopo (Fase futura)**
- Mascote visual animado / lip-sync (OpenHuman tem; exige trabalho de desktop).
- Persona de voz (TTS) afinada.
- i18n completo além de PT/EN.

## Alternatives Considered

- **Manter system prompt vazio**: rejeitado — é a causa do Garra parecer genérico.
- **Hardcode da persona sem opt-out**: rejeitado — viola o princípio de controle do
  operador e quebraria quem depende do tom neutro.
- **i18n pesado (fluent/gettext)**: adiado — desproporcional para PT/EN agora.
