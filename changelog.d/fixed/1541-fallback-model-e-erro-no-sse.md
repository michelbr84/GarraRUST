- **O fallback de provider usa o modelo DELE, e o SSE para de engolir o erro do
  provider (#1541, defeitos 3 e 4).** (3) O laco de fallback repassava o
  `LlmRequest` do primario intacto, `model` incluso — e `model` e o nome que
  *aquele* backend entende. Um llama-server local servindo `glm53-flash` caia, o
  openrouter recebia `glm53-flash` e respondia 400 `is not a valid model ID`; o
  usuario via `all providers failed`, como se o fallback nem existisse. Agora o
  `model` so sobrevive quando nomeia o proprio fallback (`openrouter/auto` indo
  para o openrouter); nos demais casos vira string vazia, que desde sempre
  significa "provider, use o seu modelo configurado" em todos os adaptadores —
  ou seja, o fallback passa a usar o `llm.<perfil>.model` que a config ja dizia.
  Vale nos dois lacos, batch e streaming. (4) Em `stream: true` o `Err` do turno
  era descartado por um `if let Ok(..)`: o canal de deltas fechava sem nenhum
  delta e o SSE terminava em `finish_reason: "stop"` mais `[DONE]` — uma
  resposta em branco com cara de sucesso, indistinguivel de um modelo que
  escolheu nao falar. O turno agora reporta o desfecho por um canal proprio e o
  stream emite `{"error": {...}}` (convencao que vLLM/llama.cpp/LiteLLM ja usam)
  em vez de mentir `stop`.
- **Modelo HuggingFace servido pelo Ollama para de ser mandado ao openrouter
  (#1541, defeito 2 — o que sobrou depois da #1540).** Prefixo nao registrado
  com barra ia para o openrouter, porque ele de fato proxia `minimax/...`,
  `yi/...` e outros fora da tabela de tipos. Mas
  `hf.co/unsloth/Qwen3.8-27B-GGUF:UD-Q8_K_XL` tem duas barras, e id do
  openrouter e sempre `vendor/modelo` — mandar assim era 400 garantido. Agora
  so uma barra qualifica para a tentativa via openrouter; o resto cai no
  provider default. Nenhum id real do openrouter sai do caminho.
