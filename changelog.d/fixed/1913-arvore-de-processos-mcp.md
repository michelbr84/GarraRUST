- **Servidor MCP stdio deixa de largar descendentes vivos (#1913).** Desconexao,
  restart, queda e falha de handshake so encerravam o processo que o gateway
  lancou; filhos e netos dele ficavam orfaos, e um descendente segurando o
  stdout do servidor morto escondia a queda. Agora cada servidor nasce lider do
  proprio grupo de processos e o grupo inteiro e encerrado junto — sempre com o
  lider ainda nao colhido, para um PID reciclado nunca ser atingido. No Linux,
  quem saiu do grupo (`setsid`) e achado por um marcador de ambiente por conexao
  e morto via `pidfd`, e os que o gateway adotou (PID 1 de container) sao
  colhidos em vez de virarem `<defunct>`. Cada encerramento registra motivo,
  duracao e se sobrou algum processo. Muda um comportamento: um launcher que sobe
  o servidor em segundo plano e sai passa a ser tratado como servidor que caiu —
  o servidor deve rodar em primeiro plano (`exec`).
- **Chamada MCP sem resultado deixa rastro (#1913).** Timeout, erro e chamada
  abandonada passam a registrar servidor, ferramenta, duracao e desfecho — sem
  argumentos nem resposta. O timeout e o abandono continuam cancelando so a
  propria chamada (`notifications/cancelled`), sem derrubar o servidor
  compartilhado; os testes novos fixam isso com duas chamadas simultaneas.
