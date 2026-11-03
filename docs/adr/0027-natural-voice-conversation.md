# ADR 0027 — Conversação natural por voz na interface Web

- **Estado:** aceito; caminho A implementado pela #1754 (ver [Execução](#execução-1754)); validação acústica e adaptadores speech-to-speech pendentes.
- **Data:** 2026-10-28, America/New_York.
- **Base auditada:** `7c9cdc29`; conferir o relatório de execução para o commit final implementado.
- **Issue de execução:** [#1754](https://github.com/michelbr84/GarraRUST/issues/1754).
- **Escopo:** recurso aberto e self-hosted no gateway existente; assinaturas, pagamentos e infraestrutura comercial pertencem ao repositório privado `GarraIA/GarraIA`.

## Problema e evidências locais

O usuário precisa iniciar uma chamada uma vez, conversar sem apertar botões a cada turno, interromper a resposta e trocar modelos sem perder o histórico. A existência de endpoints de transcrição e síntese, isoladamente, não entrega esse comportamento.

O workspace já contém `garraia-voice`, o handler de voz do gateway, clientes Whisper, Chatterbox e LM Studio, configurações, sessões e um runtime conversacional com WebSocket. Reutilizar esses componentes evita uma segunda aplicação ou um agente paralelo sem as permissões e ferramentas atuais. `axum` com `ws`, `reqwest` e `tokio-tungstenite` já constam das dependências.

A auditoria encontrou limites concretos:

- `crates/garraia-voice/src/pipeline.rs` processa arquivos completos, converte OGG/WAV e só sintetiza após receber a resposta inteira; é um fluxo de mensagem de voz, não uma chamada contínua.
- `crates/garraia-voice/src/tts/chatterbox_client.rs` já descobre a assinatura Gradio. O SSE desse contrato acompanha um trabalho e depois baixa o resultado completo; isso não prova streaming de amostras de áudio.
- `crates/garraia-voice/src/stt/whisper_client.rs` possui modelo `whisper-1` fixo no fallback compatível e não oferece, nessa versão, configuração independente de modelo e credencial remota.
- `services/voice/whisper_server.py` usa faster-whisper, mas seu endpoint `/transcribe` não coincide automaticamente com os dois caminhos tentados pelo cliente Rust. A compatibilidade deve ser verificada por contrato.
- `services/voice/hibiki_server.py`, em `HibikiEngine.generate`, contém um TODO de inferência e retorna um seno de 440 Hz. Um HTTP 200 desse serviço não comprova síntese de fala. O estado de saúde não pode ser usado para anunciar TTS funcional.

## Alternativas consideradas

| Critério | A — STT → LLM → TTS | B — Speech-to-speech nativo | C — Híbrida com adaptadores |
|---|---|---|---|
| Latência | Soma etapas; reduzir espera com LLM incremental e síntese de frases | Pode evitar etapas intermediárias; precisa medir o provedor real | Usa A como base e admite B quando verificado |
| Naturalidade | Controle de texto e prosódia; a transcrição perde parte da expressão original | Pode preservar pistas da fala e permitir sobreposição de escuta/resposta | Escolha explícita conforme experiência e restrições |
| Integração Rust | Reutiliza runtime e HTTP existentes | Requer protocolo de sessão, áudio e ferramentas específico | Contrato comum com estados e capacidades explícitos |
| CPU, GPU e memória | Dependem dos três modelos; inferência local pode competir pela GPU | Local exige modelo adequado; remoto transfere inferência ao provedor | Permite combinar STT/TTS locais e LLM remoto |
| Português brasileiro | Avaliar STT e TTS separadamente, com voz adequada | Depende do modelo e de sua cobertura real | Permite preservar os componentes com melhor resultado local |
| Custo | STT, tokens LLM e TTS têm unidades distintas | Tokens/áudio e duração conforme provedor | Mesmo controle de orçamento, sem migração automática |
| Interrupção | Cancelamento e descarte precisam abranger todas as etapas | Pode ter eventos nativos; reprodução e histórico ainda precisam sincronizar | Mesmo identificador de turno e política de áudio ouvido |
| Troca e contexto | Histórico textual portável; componentes independentes | Algumas alterações exigem uma nova sessão remota | Sessão GarraIA permanece estável durante transição de adaptador |
| Operação | Menor mudança sobre o código atual | Mais dependência do protocolo de um fornecedor | Complexidade cresce apenas quando um adaptador é incorporado |

As comparações de desempenho são hipóteses arquiteturais, não medições. Nenhuma qualidade de voz ou latência do hardware do usuário foi comprovada nesta pesquisa.

**Decisão:** C, começando pelo caminho A e pelo runtime existente. Não habilitar B apenas porque um catálogo declara áudio como modalidade. O adaptador deve comprovar streaming bidirecional, cancelamento, preservação de contexto, permissões e cobrança antes de aparecer como executável.

## Pesquisa de modelos e provedores

### Whisper e Chatterbox locais

O [faster-whisper oficial](https://github.com/SYSTRAN/faster-whisper) suporta execução CPU/GPU, quantização e filtro Silero VAD. Sua API de transcrição de arquivo não se transforma em reconhecimento contínuo apenas porque retorna um gerador de segmentos. Reutilizá-la por enunciado é um primeiro passo; reconhecimento incremental exige um adaptador e política de confirmação de transcrições próprios. Os benchmarks publicados pelo projeto usam máquinas e parâmetros específicos e não são resultados do GarraIA.

O [Chatterbox oficial](https://github.com/resemble-ai/chatterbox) disponibiliza Multilingual V3 e um pacote específico de português brasileiro. A integração Gradio instalada deve informar sua versão e assinatura; atualizar pesos não pode ser presumido a partir do nome do cliente. O suporte geral a português e a existência do pacote PT-BR não demonstram a qualidade da voz instalada. A primeira entrega pode sintetizar frases completas em sequência, distinguindo isso de streaming interno do modelo.

O [Hibiki-Zero oficial](https://github.com/kyutai-labs/hibiki-zero) traduz fala de francês, espanhol, português e alemão para inglês. Portanto, não é um substituto documentado de TTS conversacional PT-BR. A implementação local de tom de teste deve ser tratada como diagnóstico legado, nunca como backend de produção selecionável por padrão.

### Descoberta OpenRouter: consulta real e capacidades exatas

Consulta pública, sem chave e sem inferência paga, realizada em **2026-10-28 22:35:38 EDT**, equivalente a `2026-10-29T02:35:38Z`:

| Consulta | Quantidade observada |
|---|---:|
| `/api/v1/models` | 457 |
| `/api/v1/models?output_modalities=all` | 689 |
| `/api/v1/models?output_modalities=speech` | 38 |
| `/api/v1/models?output_modalities=transcription` | 29 |
| `/api/v1/models?output_modalities=audio` | 4 |

São snapshots reproduzíveis, não constantes do produto. A [referência de modelos](https://openrouter.ai/docs/api/api-reference/models/list-all-models-and-their-properties) confirma que a consulta sem filtro privilegia saída `text`. A descoberta deve buscar `output_modalities=all`, armazenar horário/validade e filtrar os arrays `architecture.input_modalities` e `architecture.output_modalities`, sem heurísticas pelo nome.

No snapshot, saída `audio` inclui dois modelos Lyria de música e dois modelos `gpt-audio`; portanto esse filtro não significa TTS nem telefonia. Modelos `speech` incluem Gemini TTS e ElevenLabs; `transcription` inclui Whisper e outros ASR. `supported_voices` aparece em vários TTS. Idiomas, formatos, taxas de amostragem e suporte a streaming não estão universalmente informados: ausente significa **desconhecido**. O catálogo listou `heygen/heygen-voice-1-turbo:free` com preços zero; isso não garante disponibilidade, limites ou gratuidade de outros componentes.

### Contratos OpenRouter de áudio

A [documentação TTS](https://openrouter.ai/docs/guides/overview/multimodal/tts) define `POST /api/v1/audio/speech`, com `model`, `input`, `voice` conforme provedor e `response_format`. A resposta é fluxo binário, com `Content-Type` e `X-Generation-Id`. Formatos: `pcm` ou `mp3`; Gemini/HeyGen exigem PCM e MiniMax exige MP3. Não solicitar WAV como formato universal. PCM cru exige taxa, canais e codificação comprovados pelo adaptador antes da reprodução. Vozes vêm de `supported_voices`; omitir voz só funciona quando existe padrão documentado. `speed` e `instructions` não são universais. Preços podem ser por caractere ou por segundo gerado, e outros modelos exigem leitura da unidade específica. Essa mesma documentação informa que `provider.order/only/ignore` não restringem TTS; `zdr`, `data_collection` e opções próprias têm contratos distintos.

A [documentação STT](https://openrouter.ai/docs/guides/overview/multimodal/stt) define `POST /api/v1/audio/transcriptions`. Aceita JSON com `input_audio.data` base64 e `format`, ou multipart compatível; permite `model` e `language`. Retorna transcrição e, quando disponíveis, `usage.seconds`, tokens e `usage.cost`. Multipart possui teto de 25 MB; há timeout upstream de 60 segundos, além de limites específicos. As unidades podem ser segundos ou tokens. Não há contrato bidirecional demonstrado por essa rota. `provider.order/only/ignore` também não restringem STT. Dividir áudio e limitar duração continua responsabilidade do cliente.

Já [Chat Completions com áudio](https://openrouter.ai/docs/guides/overview/multimodal/audio) recebe arquivos base64 em `input_audio` e pode produzir `delta.audio` com `stream: true`. Esse fluxo HTTP por requisição não estabelece uma sessão Realtime com barge-in. O GarraIA deve diferenciar: arquivo de entrada, resposta incremental, síntese incremental e sessão bidirecional.

#### Precisão do contrato de reprodução e descoberta

A [referência Create speech](https://openrouter.ai/docs/api/api-reference/tts/create-speech) especifica PCM de **16 bits little-endian**. Embora seu resumo mencione WAV, o enum do corpo aceita somente `mp3` e `pcm`; usar o enum. O [tutorial oficial](https://openrouter.ai/blog/tutorials/text-to-speech/) documenta parâmetros opcionais `rate` e `channels` no `Content-Type` e identifica Mistral Voxtral como MP3-only. A [receita oficial Gemini](https://openrouter.ai/docs/cookbook/audio/two-voice-podcast) demonstra `audio/pcm;rate=24000;channels=1` e encapsulamento WAV. Portanto:

- Validar MIME, taxa, canais, alinhamento de frames e tamanho antes de reproduzir; só envolver PCM com cabeçalho WAV depois dessa validação.
- Ler parâmetros do MIME quando existirem. Se ausentes, aceitar um padrão somente para contrato explicitamente verificado, como Gemini TTS 24 kHz mono; não generalizar isso para HeyGen ou outros modelos.
- Selecionar formato antes da requisição com base no adaptador documentado. Um erro de formato deve ser visível; não gerar novamente com outro formato por tentativa automática.
- Separar a lista dinâmica de modelos do pequeno conjunto de regras de protocolo verificadas. Catálogo incompleto não justifica inventar capacidades ou esconder todos os modelos novos.

No JSON observado, `supported_voices` é `string[] | null`, e `pricing` contém strings decimais. TTS pode ter `supported_parameters: []` apesar de aceitar `voice` e `response_format`. `per_request_limits: null` significa limite não informado. Consultas aos endpoints de Gemini, HeyGen e Whisper acrescentaram algumas capacidades de clonagem, mas não uma matriz universal de formatos, canais, taxas ou unidades de preço. O parser deve preservar dados opcionais, sem transformar ausência em disponibilidade ou ausência de restrições.

### Speech-to-speech externo

A [documentação OpenAI de conversações Realtime](https://developers.openai.com/api/docs/guides/realtime-conversations) descreve voz nativa, VAD e interrupção. Com WebSocket, o cliente precisa interromper reprodução e truncar a parte não ouvida no histórico; WebRTC pode delegar esse controle ao servidor. A voz da sessão Realtime não pode ser alterada depois de emitir áudio, reforçando a necessidade de separar sessão GarraIA e sessão remota. A [visão geral oficial de áudio](https://developers.openai.com/api/docs/guides/audio) também distingue GPT-Live, Realtime e pipelines de agentes textuais; os contratos não devem ser misturados por nome de modelo.

O [Gemini Live](https://ai.google.dev/gemini-api/docs/live-api) documenta WebSocket com estado, PCM16 de entrada a 16 kHz e saída a 24 kHz, interrupção, ferramentas e transcrições. É uma alternativa técnica real, mas uma credencial OpenRouter não implica acesso ao protocolo Gemini Live. Nenhuma dessas opções foi executada com credenciais ou áudio real nesta investigação. Permanecem **pendentes de adaptador e validação**, sem ativação ou custo automático.

## Transporte, captura e controle de fala

O [AudioWorklet](https://developer.mozilla.org/en-US/docs/Web/API/AudioWorklet) executa processamento de áudio fora da thread principal. Usar Worklet para captura PCM, medição de nível e segmentação, enviando inicialmente WAV mono por enunciado ao gateway; o fluxo de texto continua no WebSocket já existente. Isso oferece escuta contínua na interface, mas STT por enunciado; a interface não deve chamar a transcrição de parcial em tempo real se o backend só devolve o resultado final.

[WebSocket não fornece backpressure automático](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket). Impor limites de fila, duração e bytes; verificar `bufferedAmount` quando houver envio contínuo; descartar frames de gerações canceladas. O primeiro incremento não precisa introduzir um servidor WebRTC. Para redes móveis instáveis ou áudio bidirecional nativo, reavaliar WebRTC com medições de jitter e perda. A [documentação de transporte OpenAI](https://developers.openai.com/api/docs/guides/voice-webrtc) recomenda WebRTC para o navegador e mantém a chave no servidor. Bibliotecas candidatas Rust: [webrtc-rs](https://github.com/webrtc-rs/webrtc), com camada assíncrona, e [str0m](https://github.com/algesten/str0m), cujo núcleo Sans-I/O exige conduzir rede e relógio. São opções avaliadas, não dependências já validadas no projeto.

Solicitar `getUserMedia` somente após iniciar a chamada, em HTTPS ou localhost, com `echoCancellation`, `noiseSuppression` e `autoGainControl` quando suportados. [Cancelamento de eco é uma restrição negociada](https://developer.mozilla.org/en-US/docs/Web/API/MediaTrackConstraints/echoCancellation); pedir `true` não garante remoção perfeita do áudio reproduzido. Inspecionar as configurações efetivas e testar alto-falantes. Não desligar o microfone durante TTS, pois isso elimina barge-in.

VAD inicial deve ter limiar calibrável, histerese, duração mínima de fala, pré-buffer e silêncio final configurável. Um detector por energia é uma base limitada: ruído e eco podem acioná-lo. [Silero VAD](https://github.com/snakers4/silero-vad) oferece modelo MIT e execução ONNX, com taxas de 8/16 kHz e exemplos para navegador e Rust; considerar após medir falsos positivos. Nenhum VAD acústico, sozinho, prova que uma frase terminou semanticamente.

## Fluxo escolhido e invariantes

```text
Iniciar chamada → permissão de microfone → AudioWorklet / VAD
    → WAV limitado de um enunciado → STT autenticado no gateway
    → texto + sessão/agente/modelo → runtime WebSocket existente
    → deltas de texto → segmentador de frases → TTS autenticado
    → fila limitada → reprodução → escuta continua ativa
```

1. **Identidade estável:** o ID da sessão GarraIA e seu histórico sobrevivem à troca de STT, LLM, TTS, voz e idioma. Não recriar a conversa para aplicar configuração de áudio.
2. **Configuração por turno:** capturar a seleção ao iniciar um turno. Mudanças posteriores cancelam explicitamente o turno ou entram no próximo; nunca misturar vozes/modelos silenciosamente numa frase.
3. **Interrupção:** ao detectar nova fala, parar o áudio local imediatamente, invalidar a geração, esvaziar a fila e propagar cancelamento para LLM/TTS. Respostas tardias devem ter seu ID rejeitado. Cancelar reprodução não prova cancelamento de inferência ou cobrança upstream.
4. **Contexto fiel:** registrar interrupção e preservar a fala do usuário. Diferenciar texto gerado de áudio efetivamente ouvido; uma resposta interrompida não deve aparecer como integralmente pronunciada. Adaptação futura S2S deve reconstruir contexto a partir do histórico canônico.
5. **Síntese incremental:** emitir frases curtas conforme deltas do LLM chegam, com ordem estável e limite de concorrência. Evitar ler código, marcação e pontuação estrutural como prosa. TTS por frase não equivale a streaming interno de áudio.
6. **Encerramento:** parar tracks, desconectar nodes, fechar AudioContext, abortar fetches e cancelar streams. Erro, navegação, logout, perda de dispositivo e timeout precisam alcançar a mesma limpeza.
7. **Recuperação:** reconexão limitada e explícita, sem repetir automaticamente uma solicitação paga cujo resultado é incerto. Preservar histórico e informar estado; nunca substituir modelo/provedor por iniciativa silenciosa.

## Segurança e custos

Reutilizar autenticação, autorização, proteção de origem, sessões e credenciais existentes do gateway. O navegador recebe capacidades e nomes públicos, nunca chaves. URLs de serviços locais pertencem à configuração confiável do servidor; o corpo enviado pelo navegador não pode escolher destinos arbitrários. Não transportar segredos em query strings ou logs. Aplicar limites de tamanho, tempo, fila e sessões simultâneas, além de expiração por inatividade.

O recurso não deve gravar áudio bruto persistentemente por padrão. Buffers em memória são descartados ao concluir/cancelar. Se for necessário arquivo temporário para um cliente legado, usar nome não previsível e limpeza também nos caminhos de erro. Histórico textual segue a política existente e seus mecanismos de exclusão.

Escolher OpenRouter deve ser uma seleção explícita do usuário. Se a política exige provedor upstream específico, não alegar que os campos de roteamento ignorados o garantem: usar adaptador direto comprovado ou recusar a operação. Aplicar restrições de retenção somente quando o contrato escolhido conseguir garanti-las.

Não converter todo `pricing.prompt` para custo por milhão de tokens: preservar a unidade comprovada. Exibir preço desconhecido quando faltar informação, distinguindo estimativa e custo efetivo. A fórmula de planejamento é soma de duração STT, tokens LLM e caracteres/tokens/duração TTS conforme o modelo; custo por minuto depende do tempo efetivamente falado e do histórico reenviado. Um preço zero isolado não prova custo total zero. Limite rígido por sessão exige conhecer as unidades e reservar o teto antes da requisição; sem isso, não anunciar garantia de orçamento. Metering local não introduz assinatura ou cobrança comercial.

## Metas propostas e validação

Os números abaixo são **alvos iniciais de engenharia**, não benchmarks nem garantias. Devem ser ajustados após registrar máquina, modelos, versões, navegador, rede, taxas de amostragem e aquecimento.

| Medida | Alvo proposto | Evidência necessária |
|---|---|---|
| Fim de fala → primeiro áudio audível | p50 ≤ 1,5 s; p95 ≤ 3 s | Relógio do cliente; separar espera VAD, STT, LLM, TTS e reprodução |
| Início de interrupção detectada → silêncio local | p95 ≤ 200 ms | Captura/reprodução instrumentadas; teste com alto-falante e headset |
| Troca de configuração sem carregar pesos | confirmação ≤ 500 ms | Evento de aplicação e próximo turno usando seleção nova |
| Sessão prolongada | 30 min sem crescimento contínuo de buffers | Heap, filas e tarefas antes/depois; alternar fala, silêncio e erros |
| Encerramento | nenhuma track ativa e nenhuma tarefa órfã | Inspeção do navegador, servidor e adaptador |

Medir também taxa de cortes prematuros, transcrições falsas no silêncio/eco, WER/CER em corpus PT-BR consentido, tempo de carregamento de modelo, RSS/VRAM, utilização CPU/GPU, bytes por minuto e custo efetivo retornado. A qualidade deve incluir avaliação humana de pronúncia, pausas, nomes e correções.

Testes automatizados devem cobrir contratos HTTP, modelos incompatíveis, autenticação, limites, erros upstream, manutenção do ID de sessão, cancelamento durante cada fase e respostas atrasadas. Simulações de PCM cobrem silêncio, fala curta, pausas, ruído e mudanças de amostragem; não substituem conversa com microfone real. Até existir execução acústica documentada, classificar naturalidade, eco e latência ponta a ponta como **não validados em hardware**.

## Consequências e pendências explícitas

A entrega inicial pode oferecer uma chamada contínua com STT por enunciado e TTS por frase, aproveitando ferramentas e histórico existentes. Não deve se apresentar como equivalente acústico de uma ligação já validada. Modelos que só existem no catálogo, sem formato/adaptador comprovado, aparecem como capacidades descobertas com estado pendente, não como escolhas executáveis silenciosamente.

Permanecem incrementos próprios: streaming real de amostras quando suportado, STT parcial confirmado, VAD semântico, jitter buffer para transporte contínuo, S2S, testes móveis e orçamento rígido completo. A interface deve tornar limitações e indisponibilidades visíveis sem bloquear o chat textual existente.

## Execução (#1754)

Implementado em 2026-11-02 (America/New_York) sobre `main` `b4ecdf0d`, seguindo o
[plan 0366](../../plans/0366-voice-chat-web.md). Este ADR descreve a decisão; o
que segue registra como ela foi materializada e onde o código diverge do texto
acima.

**Entregue (caminho A):** página Voice Chat no Web Console; AudioWorklet com
reamostragem para 16 kHz; VAD por energia com piso adaptativo, pre-roll de
240 ms, início após 120 ms de voz, fim de fala configurável (0,6–1,6 s) e teto
de 30 s por enunciado; STT por enunciado (`POST /api/voice/transcribe`); turno
no runtime/`/ws` existente; TTS por frase (`POST /api/voice/synthesize`) com
fila limitada; barge-in; catálogo dinâmico local + OpenRouter; limpeza em
encerramento, erro, `pagehide`, ociosidade, segundo plano e perda do microfone.

**Decisões tomadas na implementação:**

1. **ID do turno no protocolo.** O `/ws` ganhou `turn_id` opt-in, ecoado em
   todo frame do turno. O cliente descarta o que for de outro turno; um `stop`
   com `turn_id` antigo não cancela o turno atual. Sem `turn_id` os frames são
   os de antes (invariante 3).
2. **Áudio ouvido versus texto gerado.** O `stop` leva `heard_chars` (code
   points efetivamente tocados). Interrompido durante a geração, o histórico
   grava o trecho falado e o gerado-não-ouvido separados; interrompido na
   reprodução de um turno já concluído, o próximo turno de voz recebe uma nota
   montada pelo servidor a partir do próprio texto — nada do navegador vira
   instrução de sistema (invariante 4).
3. **Porta de energia no servidor.** Antes de qualquer STT, um enunciado sem
   ≥ 80 ms acima de ≈ −44 dBFS é respondido sem chamar provedor. É guarda de
   custo, não classificador de fala: ruído alto passa (documentado em teste).
4. **Fallback do LLM visível, não removido.** O runtime aplica a cadeia
   `agent.fallback_providers` configurada pelo operador também aos turnos de
   voz. Desligá-la por turno exigiria mudar `garraia-agents`; em vez disso o
   catálogo anuncia a cadeia antes da chamada e o frame `message` de voz diz
   `provider`/`model`/`fallback`. STT e TTS não têm fallback algum.
5. **Formato de TTS por família documentada.** Gemini e HeyGen pedem PCM,
   demais famílias MP3. PCM sem `rate`/`channels` só é aceito para Gemini
   (24 kHz mono, documentado); HeyGen sem layout é erro visível. Nenhuma
   segunda geração em outro formato.
6. **Preço sem unidade inventada.** O catálogo do OpenRouter traz valores
   decimais sem unidade para fala e transcrição; a interface mostra o valor e
   "unidade não informada". Custo efetivo só quando o provedor devolve
   `usage.cost` (STT).
7. **Dica de estilo falado como mensagem de sistema no histórico.** Não
   substitui persona, ferramentas nem aprovação. O adaptador Anthropic descarta
   mensagens de sistema fora do topo, então ali a conversa funciona sem a dica.
8. **Chave do gateway no handshake do `/ws`.** O navegador não envia header no
   upgrade de WebSocket; o Voice Chat reutiliza o mesmo `?token=` do chat de
   texto (rota única onde a query é aceita, e o log do gateway só registra o
   caminho). Nenhuma credencial de provedor passa pelo navegador.

**Medido:** nada acústico. Os testes automatizados simulam PCM, microfone e
provedores (Rust: contratos HTTP/WS e unitários; Playwright: VAD, segmentador,
chamada, barge-in, troca de modelo, limpeza). As metas da tabela "Metas
propostas e validação" seguem **não validadas em hardware**; a página exibe
"Fim da fala → primeiro áudio" e a latência de STT para a validação manual.

**Pendências, em ordem:** (1) validação física com fone e alto-falante (eco
disparando barge-in é o risco principal do VAD por energia); (2) Silero VAD se
a taxa de falsos inícios medida justificar; (3) desligar o fallback do LLM por
turno de voz em `garraia-agents`; (4) STT parcial e TTS com streaming de
amostras quando houver adaptador; (5) adaptador speech-to-speech; (6) teto
monetário por chamada.
