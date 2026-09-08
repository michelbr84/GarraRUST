- **`GET /v1/models` lista os modelos que o gateway de fato serve (#1029).** A
  resposta era uma lista fixa (`gpt-4`, `gpt-3.5-turbo`, `claude-3-opus`, ...)
  que nao lia a config: um cliente OpenAI-compatible (VS Code, Continue) listava,
  escolhia um e recebia 500 do provider real. Agora a lista sai dos providers
  registrados — o modelo configurado de cada um — filtrada pela mesma regra de
  roteamento que `POST /v1/chat/completions` aplica ao campo `model`: entra o
  modelo do provider default e qualquer modelo cujo prefixo (`openrouter/auto`,
  `anthropic/...`) ja o leve ao provider certo; um nome sem prefixo num provider
  que nao e o default fica de fora, porque nenhum id o alcancaria. `owned_by`
  passa a ser o id do provider no GarraIA.
