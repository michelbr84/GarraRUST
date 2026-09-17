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
  segundo teto — 120 s de TEMPO SEM PROGRESSO, que **nao zera com evento** —
  para o caso de o pareamento nao andar; ele diz o que verificar e manda rodar
  o comando de novo. Quem renova esse teto e a FASE do pareamento: enquanto
  houver QR na tela ele nao corre, e por isso o usuario lento para pegar o
  celular nao e cortado (quem limita esse caso e o teto de 5 QRs). Um QR que
  expira sem substituto volta a arma-lo.
- **Um unico QR nao desarma mais os tetos do pareamento para sempre
  (#1238).** Com a rede caindo logo DEPOIS de o QR aparecer, os tres relogios
  do comando ficavam desligados ao mesmo tempo: o de silencio porque cada
  tentativa fracassada o realimenta; o de QR porque, expirado aquele, a
  maquina estaciona num estado onde nao ha mais nada a expirar; e o de
  progresso porque ele era um booleano de "ja progrediu alguma vez" que o
  primeiro QR ligava e nada desligava. Com os prazos de producao o comando nao
  terminava nunca — so o Ctrl+C saia. O booleano virou um relogio de
  progresso medido pela fase do pareamento.
- **O modo `serve` do gateway ganhou prazo onde o `pair` ja tinha (#1238).**
  A espera pelo fim do processo filho, o envio de comando para ele e a espera
  pelo PROXIMO EVENTO rodavam sem relogio: uma ponte que fecha a saida sem
  terminar, que para de ler o proprio stdin, ou que sobe e emudece sem
  conectar pendurava a task do canal em vez de cair no backoff de reconexao
  que existe logo acima. O prazo do laco de eventos vale so antes de conectar:
  depois disso o silencio e o estado normal de uma conta sem mensagens.
- **O `shutdown` de cortesia deixou de poder pendurar a saida (#1238).** Ele e
  o ultimo recurso de todo caminho de desistencia, o do Ctrl+C inclusive, e
  rodava sem prazo — mas escrever no stdin do filho bloqueia assim que o
  buffer do pipe enche e o filho para de ler. Agora ele tem 2 s e desiste.
- **A linha de "nova tentativa em Ns" deixou de prometer prazo ja vencido
  (#1238).** Ela truncava a divisao, entao um backoff de 1500 ms aparecia como
  "1s" e a linha ficava vencida meio segundo depois.
- **`garra whatsapp restore` deixou de ligar o canal sem provar que a sessao
  restaurada abre (#1238).** Restaurar move bytes; nao decifra nada. Com a
  `session.key` perdida ou a passphrase do cofre trocada, o comando dizia
  "Sessao restaurada", gravava `enabled = true` e saia 0 — e o gateway passava
  a pagar timeout e retry a cada boot, que e exatamente o que a ordem
  blob-antes-de-enabled existe para evitar. Agora ele abre o blob ANTES de
  mover o arquivo e, se nao abrir, diz o que aconteceu e deixa o arquivado
  exatamente onde estava. A ordem importa: provando depois do movimento, uma
  passphrase do cofre apenas ausente do ambiente — quem a exporta e rodou num
  shell sem ela — consumia o `.prev` e recebia o conselho de ler um QR novo,
  que descartaria uma sessao intacta.
- **`npm ci` estourando o prazo nao deixa mais um processo orfao (#1238).**
  Depois dos 600 s o filho era largado rodando, mexendo no mesmo
  `node_modules` que a tentativa seguinte ia recriar.
