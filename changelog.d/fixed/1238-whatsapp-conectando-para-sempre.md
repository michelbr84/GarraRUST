- **`garra whatsapp` nao fica mais em "conectando ao WhatsApp…" para sempre
  quando a ponte fala e nunca progride (#1238).** O prazo de silencio da
  revisao anterior conta desde o ULTIMO evento e zera a qualquer um deles,
  inclusive os que significam "falhei de novo". A ponte reconecta sozinha, sem
  teto, emitindo `disconnected` e `status` a cada rodada de backoff — no
  maximo a cada 30 s, sempre abaixo dos 90 s do prazo. Resultado: o prazo
  existia, funcionava, e nunca disparava, porque o proprio fracasso o
  realimentava. Quem caia nisso era o usuario sem internet, atras de portal de
  autenticacao de wi-fi, com a porta 443 bloqueada ou com o relogio do sistema
  errado: a tela parava em duas linhas e so o Ctrl+C saia. Agora cada queda
  aparece na tela com o motivo e o prazo da proxima tentativa, e existe um
  segundo teto — 120 s desde o inicio, que **nao zera com evento** — para o
  caso de nunca haver QR nem conexao; ele diz o que verificar e manda rodar o
  comando de novo. Um pareamento que chegou a mostrar QR nunca o encontra.
- **O modo `serve` do gateway ganhou prazo onde o `pair` ja tinha (#1238).**
  A espera pelo fim do processo filho e o envio de comando para ele rodavam
  sem relogio: uma ponte que fecha a saida sem terminar, ou que para de ler o
  proprio stdin, pendurava a task do canal em vez de cair no backoff de
  reconexao que existe logo acima.
- **`garra whatsapp restore` deixou de ligar o canal sem provar que a sessao
  restaurada abre (#1238).** Restaurar move bytes; nao decifra nada. Com a
  `session.key` perdida ou a passphrase do cofre trocada, o comando dizia
  "Sessao restaurada", gravava `enabled = true` e saia 0 — e o gateway passava
  a pagar timeout e retry a cada boot, que e exatamente o que a ordem
  blob-antes-de-enabled existe para evitar. Agora ele abre o blob antes de
  ligar o canal e, se nao abrir, diz o que aconteceu e manda ler um QR novo.
- **`npm ci` estourando o prazo nao deixa mais um processo orfao (#1238).**
  Depois dos 600 s o filho era largado rodando, mexendo no mesmo
  `node_modules` que a tentativa seguinte ia recriar.
