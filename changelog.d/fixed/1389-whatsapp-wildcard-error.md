- **`garra whatsapp allow '*'` diz por que nao vale (#1389).** O curinga caia no
  erro generico de caractere ("o numero so pode ter digitos"), que faz um recurso
  inexistente parecer erro de digitacao. Agora ha uma variante propria: a mensagem
  explica que este canal nao tem "autorizar todo mundo" e que o portao e
  fail-closed, identidade a identidade. O exit code segue 65 e a semantica de
  acesso aberto continua NAO implementada.
