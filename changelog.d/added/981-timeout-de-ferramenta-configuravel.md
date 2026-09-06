- **O timeout por execucao de ferramenta virou configuravel (#981).** Eram 30s
  fixos em `ExecutionBudget::padrao()`, e isso matava ferramenta que chama LLM
  por dentro: um MCP que sumariza, um `ask` aninhado, geracao de resposta
  longa. O agente recebia `tool timeout` como se fosse falha da ferramenta — o
  erro apontava para o lugar errado. Agora `GARRA_TOOL_TIMEOUT_SECS` sobrescreve,
  e o default segue 30s (zero mudanca para quem nao configurar).
- **Valor invalido avisa em vez de sumir.** `=abc` ou `=0` volta ao padrao **e
  loga**. Cair no padrao em silencio faria alguem configurar, ver o
  comportamento antigo e nao ter como saber por que.
- **Valor absurdo e aceito, com aviso.** Acima de uma hora por ferramenta o log
  diz que, se a intencao era milissegundos, o numero esta 1000x maior — mas
  usa o que foi pedido. Quem quer mesmo um turno longo tem direito ao numero
  dele; quem digitou `30000` querendo `30` descobre antes de esperar 8 horas.
- **E variavel de ambiente, e nao chave de config, por um motivo concreto:** o
  `garraia-agents` nao depende do `garraia-config`, e criar essa dependencia so
  para um `u64` acoplaria o crate de agentes ao carregador inteiro. O preco
  seria o knob nascer invisivel — pago em separado: `garra config check` agora
  lista `GARRA_TOOL_TIMEOUT_SECS` entre as env vars detectadas, entao o
  operador descobre sem ler codigo.
