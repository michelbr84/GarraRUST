- O aviso `Update available` deixou de disparar para binario MAIS NOVO que a
  ultima release publicada (`v0.4.3 -> v0.4.2`): a comparacao passa a ser
  numerica em `X.Y.Z` e so anuncia quando a release e maior; forma
  inesperada (pre-release, sufixo) cai na desigualdade de antes, para o aviso
  nunca sumir por formato (#1320).
