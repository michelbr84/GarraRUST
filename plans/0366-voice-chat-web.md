# Plan 0366 — Voice Chat contínuo na Web (#1754)

- **Estado:** em execução (2026-11-02, America/New_York).
- **Issue:** [#1754](https://github.com/michelbr84/GarraRUST/issues/1754) (P1).
- **Decisão de arquitetura:** [ADR 0027](../docs/adr/0027-natural-voice-conversation.md) —
  caminho C começando pelo A: STT por enunciado → runtime/WS existente → TTS por frase.
- **Base:** `main` em `b4ecdf0d`, já com o #1768 (TTS da Web toca o WAV) e o #1797
  (TTS sem temporário compartilhado). O trabalho não commitado do Codex
  (`codex/natural-voice-conversation`, base `7c9cdc29`) entra como primeiro commit, rebaseado.

Este documento é o *spec* curto que o ADR pede antes do código. O ADR decide o
quê e o porquê; aqui ficam o contrato, os limites e a ordem de execução.

## 1. Escopo

Entra:

1. Página **Voice Chat** no Web Console existente (`webchat.html`, Garra Glass,
   tokens `--garra-*`, sem CDN nova), com estados visíveis, microfone pedido só
   ao iniciar, silenciar, interromper, encerrar e limpeza completa.
2. Captura por **AudioWorklet** (PCM mono 16 kHz) + **VAD por energia** com
   pre-roll, histerese, fim de fala configurável e teto de 30 s por enunciado.
3. **Barge-in**: fala nova para o áudio local na hora, esvazia filas, aborta
   fetches de STT/TTS e cancela o turno do LLM no `/ws`; frames e respostas
   atrasadas são descartados pelo **ID do turno**.
4. Rotas HTTP autenticadas: `GET /api/voice/models`, `POST /api/voice/transcribe`,
   `POST /api/voice/synthesize` — destino e credencial vêm só da config do servidor.
5. Seleção **independente** de STT, LLM (provedor + modelo), TTS, voz e idioma,
   aplicada no início de cada fala, sem recriar a sessão.
6. Catálogo dinâmico: config local + OpenRouter `GET /models?output_modalities=all`
   filtrado por `architecture.{input,output}_modalities`; preço como o provedor
   informa, unidade "não informada" quando o catálogo não diz; indisponibilidade
   com motivo; speech-to-speech listado como **não executável**.

Fica de fora (pendência explícita do ADR): STT parcial/streaming, streaming
interno de amostras de TTS, adaptador speech-to-speech (Realtime/Gemini Live),
WebRTC, VAD neural (Silero), orçamento rígido por chamada, faturamento.

## 2. Contrato

### 2.1 HTTP (`crates/garraia-gateway/src/voice_chat.rs`)

| Rota | Entrada | Saída | Recusa antes de gastar |
|---|---|---|---|
| `GET /api/voice/models` | — | `{stt[], tts[], speech_to_speech[], limitations[], llm_fallback_providers[], catalogs[], default_language}` | — (descoberta não faz inferência) |
| `POST /api/voice/transcribe` | multipart: `file` (WAV PCM16 mono 16 kHz, ≤ 30 s), `provider`, `model`, `language?` | `{text, usage, skipped?}` | WAV fora do contrato → 400; enunciado sem energia de fala → 200 `skipped:"silence"` sem chamar STT; campo desconhecido → 400 |
| `POST /api/voice/synthesize` | JSON `{text ≤ 4 KiB, provider, model, voice?, language?}` (`deny_unknown_fields`) | `audio/wav` ou `audio/mpeg`, `cache-control: no-store` | modelo sem a capacidade, voz não anunciada, campo `endpoint`/`api_key`/… → 4xx |

Regras comuns: gate de `gateway.api_key` e `cross_origin_guard` (já montados no
router), semáforo global (4 sínteses/transcrições, 2 descobertas → 429),
timeout por fase (60 s), corpo upstream limitado, sem redirect, sem eco do
corpo do provedor, **sem retry e sem provedor alternativo**. `provider: "local"`
usa os clientes do `--with-voice`; o Hibiki legado aparece como indisponível
(o servidor de exemplo devolve um tom, não fala).

### 2.2 WebSocket (`/ws`, aditivo e opt-in)

- Mensagem: `{content, provider, model?, stream: true, voice: true, turn_id}`.
  `turn_id` tem 1–64 caracteres `[A-Za-z0-9_-]`; inválido → frame `error`
  `invalid_turn_id`, sem turno.
- Todo frame do turno (`delta`, `tool_*`, `message`, `error`, `stopped`) leva o
  `turn_id` quando a mensagem trouxe um. Sem `turn_id`, os frames são
  byte-idênticos aos de antes.
- `message` de turno de voz leva `provider`, `model` e `fallback` lidos de
  `last_turn_stats`: se o fallback configurado pelo operador responder, a UI
  mostra — nada troca de provedor em silêncio.
- `stop`: `{type: "stop", session_id?, turn_id?, heard_chars?}`. Um `stop` cujo
  `turn_id` não é o do turno ativo **não** cancela o turno ativo (é de um turno
  velho): responde `stopped` com o `turn_id` dele.
- Interrupção durante a geração: persiste a fala do usuário e o parcial marcado
  como interrompido, separando o trecho falado (`heard_chars`) do gerado e não
  ouvido. Interrupção durante a reprodução de um turno já concluído: o próximo
  turno de voz recebe uma nota de contexto montada **pelo servidor** a partir do
  texto que ele próprio gerou (nada do navegador vira instrução de sistema).
- Turno de voz: `max_tokens` 1024, timeout de 120 s, dica de estilo falado
  acrescentada ao histórico (não substitui persona, ferramentas nem aprovação).

### 2.3 Cliente (`assets/voice-chat.js`)

- Estado: `idle → connecting → listening ⇄ user-speaking → processing → speaking`,
  mais `muted`, `reconnecting`, `error`.
- Cada fala ganha `turn_id` novo; frames com outro `turn_id`, respostas de
  STT/TTS de geração velha e áudio decodificado tarde são descartados.
- Fila de TTS limitada (8 frases, 2 áudios prontos); falha de síntese mostra
  erro e mantém a transcrição — nenhum provedor alternativo é acionado.
- Encerramento (botão, `pagehide`, 30 s em segundo plano, 5 min ocioso, 60 min
  de chamada, perda do microfone, falha de reconexão): para tracks, desconecta
  nodes, fecha o `AudioContext`, aborta fetches, fecha o socket.
- Seam de teste `window.__garraVoice` (congelado), no padrão do `__garraTts`.

## 3. Segurança e custo

Navegador nunca recebe chave de provedor nem escolhe URL; política de roteamento
upstream não suportada pelas rotas de áudio é recusada (só `zdr` e
`data_collection`). Áudio só em memória; nenhum arquivo é gravado pelo Voice
Chat. Logs sem texto do usuário em `info`. Preço exibido é metadado do
provedor; custo efetivo só quando a resposta traz `usage.cost`.

## 4. Testes

- **Rust** (`garraia-voice`): timeouts, limites de corpo, sem fallback em
  timeout, concorrência sem troca de áudio (do #1797), WAV/PCM.
- **Rust** (`garraia-gateway`): `voice_chat_contract.rs` (auth, origin, limites,
  catálogo por fixture local, capacidade, voz, erro upstream sanitizado, sem
  redirect, sem retry, 429 de concorrência, PCM→WAV, silêncio não chama STT);
  `voice_ws_contract.rs` (turn_id nos frames, stop de turno velho não cancela o
  atual, sessão mantida ao trocar provedor/modelo, contexto interrompido);
  unitários do detector de energia e do WAV com PCM simulado.
- **Browser** (`tests/playwright/webchat-voice-chat.spec.ts`): VAD e
  segmentador com PCM simulado; chamada completa com microfone falso, `/ws`
  roteado e rotas de voz simuladas — fala → transcrição → deltas → síntese por
  frase → barge-in → frames velhos ignorados → troca de modelo na mesma sessão
  → silenciar → encerrar com limpeza.
- **Físico (não automatizável aqui):** checklist no PR para o dono.

## 5. Ordem de execução

1. Rebase do WIP sobre a `main`, preservando #1768/#1797; compilar e rodar voz.
2. Servidor: WS (`turn_id`, stop por turno, contexto interrompido, quem
   respondeu); HTTP (catálogo com horário/preço/unidade, silêncio, PCM Gemini).
3. Cliente: `turn_id`, `heard_chars`, quem respondeu, preço, nova conversa, seam.
4. Testes Rust e Playwright; docs (`docs/voice.md`, ADR 0027 §execução);
   fragmentos de changelog.
5. PR único, CI, relatório verificável.
