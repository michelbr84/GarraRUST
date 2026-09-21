- **Loop detector: o erro de tool em loop traz o diagnostico do input repetido (#1295, item 2 de 3).**
  Em vez do seco "tool loop detected: <tool>", o erro agora leva o nome da
  ferramenta, a contagem da janela (3 chamadas identicas em sequencia) e o
  input repetido, com segredo redigido e truncado a 200 caracteres — e quem
  le (o LLM no proximo turno, ou o humano no log) tem como corrigir. A
  mensagem nasce num unico lugar (`ExecutionBudget::mensagem_de_loop`) e
  passa pelo despacho unico do #1311; o gatilho (3 assinaturas identicas)
  nao mudou.
