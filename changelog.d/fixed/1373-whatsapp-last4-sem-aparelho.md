- **O log do WhatsApp vinculado mostra o final certo do numero conectado
  (#1373).** `Jid::last4` contava todos os digitos do JID, inclusive o sufixo
  de aparelho: `5511555554321:7@s.whatsapp.net` virava `phone_last4=3217`, o
  final de um numero que nao existe, justamente na linha em que o operador
  confere qual conta conectou. Agora so a parte de usuario conta (antes do `:`
  e do `@`), o mesmo corte do `phoneLast4` da ponte, para JID de telefone,
  `@lid`, grupo e JID sem dominio.
