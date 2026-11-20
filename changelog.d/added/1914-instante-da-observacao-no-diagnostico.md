- **`garra_diagnostics` diz quando o relatorio foi observado (#1914).** O
  relatorio do operador ganha `observado_em` (UTC ISO 8601 com `Z`, truncado
  no segundo) ao lado de `verificacoes`: o instante em que a coleta comecou,
  e o mesmo de onde `processo.servico` deriva o `iniciado_em`. E o que deixa
  comparar dois relatorios antes/depois de um update sem depender do relogio
  de quem leu. A recusa continua o minimo absoluto, so com `verificacoes`.
