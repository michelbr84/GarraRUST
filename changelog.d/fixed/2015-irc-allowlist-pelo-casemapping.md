- **A allowlist do IRC reconhece a mesma pessoa que a sessao (#2015).** A
  sessao privada usa o nick normalizado pelo `CASEMAPPING` do servidor desde a
  #2011, mas a allowlist comparava o nick com a caixa exata: `Ana` admitida era
  barrada em silencio ao voltar como `ana` (e `[bia]` como `{bia}` sob
  `rfc1459`), e precisava de outro codigo de `/pair`. Agora as entradas do IRC
  sao `irc:<nick como veio>`, comparadas pelo `CASEMAPPING` anunciado quando a
  mensagem chega (sem anuncio, `ascii`), e as dos outros canais continuam
  exatas. Uma entrada antiga, sem prefixo, vale exata como antes e ganha a forma
  `irc:` na primeira admissao; para tirar alguem do IRC, apague as duas.
