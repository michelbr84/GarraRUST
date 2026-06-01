# Plan 0250 — Garra Persona Amistosa (inspirado em OpenHuman)

> **Linear:** GAR-771 (a criar) — Epic "Garra Friendly Persona"
> **Branch:** `claude/garra-friendly-persona`
> **Autor:** sessão autônoma 2026-06-01 (Florida local time)
> **Status:** 🔄 In Progress
> **ADR relacionado:** 0012 (Persona & Tom de Voz do Garra) — criado neste plano

## 1. Contexto e Motivação

O usuário pediu para deixar o **Garra mais amistoso**, no espírito do
[OpenHuman](https://github.com/tinyhumansai/openhuman) — um assistente de IA
pessoal cujo diferencial declarado é ser **"Simple, UI-first & Human"**,
*"Built with the human in mind"*, com onboarding sem fricção (*"no config-first
setup, no terminal required"*), memória relacional (*"remembers you across
weeks"*) e uma presença calorosa (mascote com "rosto", voz, emojis,
linguagem conversacional).

### Diagnóstico do estado atual do Garra

Mapeamento das "superfícies de personalidade" hoje (todas verificadas no código):

| Superfície | Arquivo | Estado atual | Problema |
|------------|---------|--------------|----------|
| Persona default do agente | `garraia-agents/src/runtime.rs:578-585` | `system_prompt` → `unwrap_or_default()` = **string vazia** | **Garra não tem personalidade nenhuma por padrão** — responde genérico do modelo base |
| `/start` (boas-vindas) | `garraia-channels/.../builtins/start.rs:22` | `"Welcome to GarraIA! Send me a message and I will respond."` | Seco, em inglês, transacional, sem calor |
| `/help` | `garraia-channels/.../builtins/help.rs:19` | `"GarraIA Commands:\n/help - show this help..."` | Lista técnica, sem tom |
| Banner CLI | `garraia-cli/src/banner.rs:124,140` | `"Welcome to GarraIA!"` / `"Rust · Personal AI"` | OK visualmente (Ferris), mas frio |
| Wizard | `garraia-cli/src/wizard/mod.rs:60` | `"GarraIA Setup Wizard"` + prompts seções seca | Onboarding funciona mas não acolhe |
| Erros / fail states | vários | mensagens técnicas (ex.: "no API key", "503") | Intimidam usuário novo |

**Conclusão central:** o maior gap não é visual — é que **Garra não fala como
o "Garra"**. Sem `system_prompt`, ele herda o tom neutro do modelo. O wizard
até sugere *"Seu nome é Garra, um assistente pessoal..."* como exemplo, mas
isso é opcional e a maioria dos usuários deixa vazio.

## 2. Princípios de Design (o "tom Garra")

Inspirado no OpenHuman, mas com identidade própria (PT-BR primeiro, brasileiro,
caloroso sem ser bajulador):

1. **Humano, não robótico** — fala na primeira pessoa ("eu", "vou te ajudar"),
   usa contrações naturais, evita jargão.
2. **PT-BR nativo** — o público primário é brasileiro. Copy default em
   português, com fallback EN quando `lang=en`.
3. **Caloroso, conciso** — acolhe sem encher linguiça. Uma saudação curta vale
   mais que um parágrafo.
4. **Emojis com parcimônia** — 🦴/🐾/👋 pontuais (a "Garra" remete a um animal
   leal); nunca poluir.
5. **Proativo e tranquilizador em erros** — todo erro vira "aqui está o que
   aconteceu + o próximo passo", nunca um código cru.
6. **Respeita o usuário** — nada de bajulação vazia ("Que ótima pergunta!").
   Direto, gentil, competente.

> A personalidade é **configurável e desativável**: tudo entra como *default*,
> mas o usuário pode sobrescrever via `agent.system_prompt` no config ou
> `agent.persona = "neutral"`. Nunca forçamos.

## 3. Escopo (slices)

Entrega incremental, cada slice é um PR independente e testável.

### Slice 1 — Persona default (o coração) ⭐
**Objetivo:** Garra ganha uma voz própria por padrão.

- Novo módulo `garraia-agents/src/persona.rs`:
  - `pub const DEFAULT_PERSONA_PT: &str` — system prompt caloroso em PT-BR
    (nome "Garra", assistente pessoal leal, tom do §2).
  - `pub const DEFAULT_PERSONA_EN: &str` — equivalente em inglês.
  - `pub fn default_persona(lang: Lang) -> &'static str`.
- `runtime.rs`: quando `effective_system_prompt` for `None`/vazio, usar
  `default_persona(...)` em vez de string vazia. Preserva 100% override do
  usuário (config/CLI continuam ganhando).
- `garraia-config`: novo campo `agent.persona: Option<PersonaMode>`
  (`friendly` default | `neutral` | `custom`). `neutral` = comportamento antigo
  (vazio). `custom` = usa `system_prompt`.
- Testes: default aplicado quando vazio; override respeitado; `neutral` volta ao
  vazio; PT vs EN.

### Slice 2 — Copy de boas-vindas e ajuda
**Objetivo:** primeiras telas que o usuário vê soam como o Garra.

- `start.rs`: nova saudação calorosa em PT-BR (com EN fallback), apresentando
  o Garra na primeira pessoa + 3 sugestões de primeira interação ("experimente
  me perguntar...").
- `help.rs` + registry: cabeçalho amistoso, comandos agrupados com 1 linha de
  contexto, tom acolhedor.
- i18n leve: helper `persona_copy(lang)` em `garraia-channels` (sem dependência
  pesada de i18n; só PT/EN match).
- Testes: snapshot do texto PT e EN; presença do nome "Garra".

### Slice 3 — Wizard e banner acolhedores
**Objetivo:** onboarding "OpenHuman-like" — menos fricção, mais calor.

- `wizard/mod.rs`: abertura ("Oi! Vamos configurar o Garra juntos — leva 1
  minuto 🐾"), microcopy de cada seção reescrita, mensagem final celebrativa
  ("Pronto! O Garra está de pé. 🎉").
- `banner.rs`: tagline `"Rust · Personal AI"` → `"Seu assistente pessoal"` +
  linha de "primeiro passo" quando nenhum provider está ativo.
- **Persona default no wizard:** quando o usuário deixa o system prompt vazio,
  gravar a `DEFAULT_PERSONA_PT` no config (em vez de `None`), para que a
  experiência "amistosa" seja a padrão de fato. (Conecta com Slice 1.)
- Testes: o `config_writer` grava persona default quando prompt vazio.

### Slice 4 — Erros humanizados (fail-soft friendly)
**Objetivo:** estados de erro deixam de intimidar.

- Helper central `friendly_error(kind) -> String` para os casos mais comuns que
  um usuário novo encontra:
  - provider sem API key → "Ainda não tenho uma chave de API configurada pra
    falar com o modelo. Rode `garra config` ou defina a variável X."
  - vault sem passphrase (o caso real do usuário!) → "Suas credenciais estão
    no cofre, mas preciso da senha pra abri-lo. Defina `GARRAIA_VAULT_PASSPHRASE`
    e me reinicie."
  - Telegram sem token → mensagem clara do próximo passo.
- Aplicar no `bootstrap` (warns) e onde o usuário lê (CLI).
- **Bônus (resolve dor real da sessão):** aviso explícito no boot quando o
  vault existe mas a passphrase não está no ambiente.
- Testes: cada `friendly_error` contém o próximo passo acionável.

### Slice 5 — ADR + docs + ROADMAP
- `docs/adr/0012-garra-persona.md` — registra a decisão de persona/tom.
- Atualizar `README` (seção "Conheça o Garra"), `ROADMAP` (item entregue),
  `plans/README.md`.

## 4. Não-escopo (por ora)

- Mascote visual animado / lip-sync (OpenHuman tem; é Fase futura, exige
  trabalho de desktop/Tauri grande — fora deste plano).
- Voz/TTS de personalidade (já existe `garraia-voice`; tuning de persona de voz
  fica para depois).
- i18n completo multi-idioma (só PT/EN agora).

## 5. Invariantes / Regras

- **Zero breaking change:** `persona=neutral` reproduz exatamente o
  comportamento atual (system prompt vazio).
- **Override do usuário sempre vence** (config `system_prompt` / CLI flag).
- **Sem `unwrap()` em produção**; `cargo clippy --workspace` limpo.
- **Sem segredo em copy/log.**
- Copy default em PT-BR; EN como fallback paramétrico.
- Cada slice: `cargo check -p <crate>` + testes verdes antes do commit.

## 6. Ordem de execução

Slice 1 → 2 → 3 → 4 → 5. Slice 1 é pré-requisito de 2/3. Cada um vira um commit
(ou PR) próprio. Para esta sessão, implementaremos **Slices 1–3 + 5** (núcleo da
persona amistosa) e deixaremos Slice 4 (erros) como follow-up se o tempo apertar
— mas o aviso do vault (dor real) entra já no Slice 3.

## 7. Critério de Sucesso

- Instalar do zero, rodar `garra` sem configurar system prompt → Garra se
  apresenta com nome e tom caloroso em PT-BR.
- `/start` e `/help` no Telegram soam acolhedores.
- Wizard guia com microcopy amistosa e celebra no fim.
- `persona=neutral` restaura o comportamento antigo (test-provado).
- CI verde; merge em `main`.
