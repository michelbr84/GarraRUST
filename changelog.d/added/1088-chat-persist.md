- **`garra chat` ganha `--persist` e `--resume <SESSION_ID>` para nao perder a
  conversa quando o processo acaba (#1088).** O REPL guardava o historico so
  na memoria, entao a conversa morria com ele; o que decidia isso era um
  comentario em `chat.rs`, sem flag nem doc. Agora e opt-in explicito: sem
  nenhuma das duas flags **nenhum** banco e aberto e nada vai para o disco —
  exatamente o comportamento anterior, e ha teste que prende isso.
- Com `--persist`, cada turno vai para `<data_dir>/sessions.db` (o mesmo
  `sessions.db` do gateway) por `upsert_session` + `append_message`, sem
  mudanca de schema; o id da sessao aparece na abertura ja com o comando para
  retoma-la. Com `--resume`, o historico gravado e carregado antes do primeiro
  turno, a tela diz quantos turnos voltaram e a sessao continua gravando dali
  em diante — as duas flags sao independentes, mas qualquer uma basta para
  abrir o store. Falha de gravacao avisa e a conversa segue.
