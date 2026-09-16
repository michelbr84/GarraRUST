- **O auto-router de modo passa a valer em todos os pontos de entrada
  (#1223).** O roteador do GAR-227 — heuristica primeiro, LLM curto depois —
  ja existia e funcionava, mas estava ligado em um lugar so: o shim OpenAI do
  gateway. `garra chat`, `POST /api/chat`, o WebSocket do webchat, o app mobile
  e os treze canais (Telegram, Discord, Slack, WhatsApp, Signal, Matrix,
  iMessage, IRC, Line, Google Chat, Teams, OpenClaw) nunca classificavam nada:
  a sessao sem modo escolhido seguia sem modo, e o trabalho do roteador so
  aparecia para quem entrava pela porta que quase ninguem usa.
  A classificacao passou para um unico ponto de estrangulamento,
  `AppState::exec_context_for_msg`, que so age quando as tres condicoes valem
  ao mesmo tempo: a sessao nao tem modo escolhido, a flag
  `agent.auto_router_llm_enabled` esta ligada, e ha texto de mensagem. O
  `exec_context_for` antigo continua existindo e delega com `text: None`, entao
  os caminhos sem mensagem disponivel (a2a, entre outros) nao mudam.
  O contrato do GAR-227 fica intacto no ponto que mais importa: o modo deduzido
  e persistido com `set_agent_mode_auto`, aparece no `/mode` e **nao** liga a
  ToolPolicy do #988. Deduzir nao e consentir — um modo que o sistema adivinhou
  nao concede tool nenhuma que o usuario nao tenha concedido.
  Com a flag desligada, que segue sendo o default, o comportamento e byte a
  byte o de antes; ha teste de regressao afirmando exatamente isso.
