- **`/goal` existe de verdade (#983).** `/goal <texto>` define o objetivo da
  sessao, `/goal` consulta, `/goal clear` remove. O objetivo persiste entre
  mensagens e entre reinicios, e nao vaza entre sessoes.
- **O runtime recebe o objetivo explicitamente**, num campo do `ExecContext`, e
  nao concatenado na mensagem — que e o que o criterio de aceite pede. A
  diferenca e pratica: concatenado na mensagem, o objetivo sumiria da janela
  junto com ela quando o historico fosse podado. Como enquadramento do turno,
  ele entra no prompt de sistema e fica.
- **O objetivo e por pessoa, e nao por sessao — e isso e seguranca, nao
  preferencia.** Achado ALTO de auditoria: em grupo do Telegram ou do iMessage a
  chave da sessao e do **canal** (`external_id = chat_id`), entao todos os
  membros compartilham uma sessao. Como o objetivo entra no prompt de
  **sistema**, um objetivo por sessao deixaria qualquer membro escrever
  instrucao de sistema para os turnos dos outros — com um comando `Role::User`,
  sem eles saberem. Em conversa de um para um nada muda: a sessao tem uma pessoa
  so. Objetivo compartilhado de time e outra funcionalidade, e precisaria de
  permissao explicita para quem define.
- **O texto tem teto de 2000 caracteres, e passar disso e recusa e nao
  truncamento.** O objetivo volta no prompt de sistema de **todo** turno
  seguinte, entao um `/goal` de 10 MB seria custo de token recorrente — e, em
  canal de grupo, um membro escolheria esse custo para o canal inteiro.
- O objetivo mora no mesmo metadado de sessao que o modo. Isso so e seguro desde
  a correcao do upsert (#1008): antes, a gravacao do turno substituia a coluna
  inteira, entao gravar objetivo ali seria gravar e perder no mesmo turno —
  exatamente o que acontecia com o `/mode`. Ha teste para os dois convivendo.
