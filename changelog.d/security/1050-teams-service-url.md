- teams: o `serviceUrl` que decide para qual host a resposta vai — com o
  bearer do bot junto — vinha do corpo da requisicao e era usado sem
  verificacao. Agora ele so vale se casar com a claim `serviceurl` assinada
  pela Microsoft no proprio token, e o POST de saida ainda passa pelo
  `garraia_common::ssrf` (https-only, faixas publicas, IP pinado), que barra
  169.254.169.254 e a rede interna mesmo para uma URL legitimamente assinada.
  O `conversation_id`, que tambem vem do corpo, passou a ser percent-encodado
  em vez de concatenado no caminho. O canal nao tinha rota ate agora, entao
  nenhuma instalacao esteve exposta. (#1050)
