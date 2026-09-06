- **O terminal mostra o que o agente esta fazendo, ferramenta por ferramenta
  (#937).** Ate aqui a execucao de ferramenta era invisivel no `garra chat`:
  o usuario via o indicador de atividade e, minutos depois, a resposta pronta —
  sem saber se o agente estava lendo um arquivo, rodando `cargo test` ou parado.
  Agora cada chamada aparece em duas linhas compactas: `● Bash cargo test` ao
  comecar e `└─ 148 passed · 6.3s` ao terminar, com glifo e cor diferentes na
  falha (`× Bash └─ error: exit 101 · 4.2s`). Saida longa **nao** e despejada:
  vira a primeira linha mais a contagem das outras.

  Por baixo, o `garraia-agents` ganha `TurnEvent` e `TurnSink`. O runtime
  passa a contar o turno inteiro — texto e ciclo de vida de ferramenta — num
  canal so, o que preserva a ordem entre os dois (dois canais nao garantiriam,
  e o agente intercala texto e ferramenta no mesmo turno). Os sete consumidores
  que so querem texto — Telegram, Slack, Discord, WhatsApp, `openai_api`,
  `parrot_ws` e o `garra ask` — ficam intactos: num sink de texto os eventos de
  ferramenta sao descartados na origem.

  O resumo do input e o do output passam por `redact_secrets` **antes** de
  virar evento, e nao no renderer: qualquer consumidor futuro herda a garantia
  sem precisar lembrar dela.

- **O indicador de atividade volta depois que a ferramenta termina (#937).**
  Ele parava no primeiro token e nao voltava mais, entao o tempo em que o
  modelo pensa **depois** de uma ferramenta era prompt morto de novo. Faltava
  o evento para ouvir; agora existe. O rotulo `Garra` continua saindo uma vez
  so por turno.

- **O redactor de segredos aprendeu os segredos de terceiro (#937).** Ate aqui
  ele so via log, onde aparecem as chaves que o proprio GarraIA usa — Anthropic,
  OpenAI, Slack, Discord. Com os eventos de ferramenta ele passou a ver
  **comando que o agente monta**, e ali entra credencial que o usuario deu no
  contexto. Foram acrescentados: PAT do GitHub (classico e fine-grained), JWT,
  access key da AWS (fixa e temporaria), token de bot do Telegram e senha
  embutida em connection string (`postgres://user:senha@host`). Como o
  `redact_secrets` e o mesmo que o `RedactingWriter` usa, todo log do projeto
  ganhou a cobertura junto. O que ele continua **nao** cobrindo, e esta escrito
  no codigo: segredo sem formato reconhecivel, como `--password minhasenha` —
  um regex que tentasse pegar "o argumento depois de --password" erraria mais
  do que acertaria.
