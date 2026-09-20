- `/model` no chat agora e transacional: valida o identificador contra o
  catalogo real do provider (nova surface `validar_modelo` em `LlmProvider`;
  o OpenRouter consulta a lista completa `GET /models`, nao a curada de
  populares) antes de alterar o estado. Modelo ausente, provider sem resposta
  ou erro de rede recusam a troca e mantem provider/model anteriores, com
  mensagem dizendo o estado vigente. Rota valida fora da lista curada (ex.:
  `z-ai/*`, o padrao do ADR 0022) aplica com nota explicita de que nao vem do
  `/models`; provider sem catalogo aplica com ressalva de troca nao validada.
