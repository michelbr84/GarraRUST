- **`/goal` existe de verdade (#983).** `/goal <texto>` define o objetivo da
  sessao, `/goal` consulta, `/goal clear` remove. O objetivo persiste entre
  mensagens e entre reinicios, e nao vaza entre sessoes.
- **O runtime recebe o objetivo explicitamente**, num campo do `ExecContext`, e
  nao concatenado na mensagem — que e o que o criterio de aceite pede. A
  diferenca e pratica: concatenado na mensagem, o objetivo sumiria da janela
  junto com ela quando o historico fosse podado. Como enquadramento do turno,
  ele entra no prompt de sistema e fica.
- O objetivo mora no mesmo metadado de sessao que o modo. Isso so e seguro desde
  a correcao do upsert (#1008): antes, a gravacao do turno substituia a coluna
  inteira, entao gravar objetivo ali seria gravar e perder no mesmo turno —
  exatamente o que acontecia com o `/mode`. Ha teste para os dois convivendo.
