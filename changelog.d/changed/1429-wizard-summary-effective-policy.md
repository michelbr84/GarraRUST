- **O `garra whatsapp link` termina mostrando a politica de acesso em vigor (#1429).**
  O wizard fechava com "pronto para receber mensagens" sem dizer quem, afinal, podia
  falar com o GarraIA por aquele WhatsApp: as contagens so apareciam no meio do
  fluxo, e quem acabara de parear (ou de re-vincular um aparelho com `allow` herdado)
  saia sem ver o portao. Agora, antes da linha final, ele imprime o MESMO resumo do
  `garra whatsapp access` — canal, perfil de execucao, admissao, default do
  desconhecido, grupos, contagens e cada principal com piso, nivel e o que pode de
  fato, identidades so pelos quatro ultimos digitos, nunca o numero inteiro. E a
  mesma funcao de formatacao da outra tela, e nao uma segunda copia; com o portao
  vazio o aviso de "ninguem autorizado" sai uma vez so, como linha final.
