- **WhatsApp: mensagem enviada durante o restart do gateway deixa de sumir
  (#1814).** O Baileys entrega o que ficou na fila do servidor com a ponte fora
  como `append`, e a ponte so aceitava `notify`: todo "oi" mandado enquanto o
  gateway reiniciava (inclusive o restart do fim do assistente do Desktop) era
  perdido sem log. Agora a mensagem de terceiro que chegou "offline" e
  respondida se tiver ate 10 minutos; as mais velhas viram descarte com motivo
  `offline_expired`, para o Garra nao acordar respondendo conversa antiga.
