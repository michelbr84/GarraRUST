- **O `garra whatsapp link` termina mostrando o acesso em vigor (#1429, parcial).** O
  wizard fechava com "pronto para receber mensagens" sem dizer quem, afinal, podia
  falar com o GarraIA por aquele WhatsApp: as contagens so apareciam no meio do
  fluxo, e quem acabara de parear (ou de re-vincular um aparelho com `allow` herdado)
  saia sem ver o portao. Agora, antes da linha final, ele imprime o MESMO resumo do
  `garra whatsapp users` — canal ligado ou nao, contagens e as identidades por
  papel e pelos quatro ultimos digitos, nunca o numero inteiro. E a mesma funcao de
  formatacao das outras duas telas, e nao uma terceira copia; com o portao vazio o
  aviso de "ninguem autorizado" sai uma vez so, e nao repetido como linha final.
  Entrega **parcial** da #1429: modo de admissao (restrito/aberto) e niveis
  Chat/Read/Full/Write nao entram aqui — dependem da #1388 (WhatsApp Access Policy
  v2), #1390 e #1392, e mostra-los antes disso prometeria um controle que o portao
  do gateway nao aplica.
