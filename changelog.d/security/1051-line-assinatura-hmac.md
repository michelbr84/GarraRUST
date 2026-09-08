- **A assinatura do webhook do LINE passa a ser verificada de verdade (#1051).**
  `validate_signature` era um stub com `TODO` que devolvia `true` para qualquer
  entrada: quem descobrisse a URL do webhook podia forjar mensagem como se
  viesse do LINE, porque essa assinatura e a unica prova de autenticidade que o
  protocolo oferece. O modulo novo `line_channel::signature` implementa o
  contrato real — `Base64(HMAC-SHA256(channel_secret, corpo_cru))` comparado com
  o header `X-Line-Signature` em tempo constante (`subtle::ConstantTimeEq`),
  sobre a mesma pilha RustCrypto ja usada em `garraia-auth` e `garraia-storage`.
  Base64, nao hex: um digest de 32 bytes da 44 chars em Base64 e 64 em hex, e o
  formato errado recusaria toda requisicao legitima. `LineChannel::new` passou a
  devolver `Result` e recusa `channel_secret` vazio, entao o canal nao chega a
  existir sem o segredo — a checagem fica no construtor, e nao no bootstrap,
  para que nenhum wiring futuro consiga pular. O canal LINE continua sem chegar
  ao gateway (#1050), entao o impacto hoje e nulo; isto e a precondicao que
  faltava para liga-lo.
