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
- **O pareamento que autentica e nunca conecta tambem tem teto agora
  (#1238).** O relogio de progresso da revisao anterior era renovado enquanto
  o pareamento PERMANECIA numa fase que anda — e permanecer e justamente o que
  permite estacionar. Bastava a fase "autenticado", que nao expira sozinha e
  nao sai com uma queda, para o comando rodar para sempre: e o caminho real do
  WhatsApp, em que logo depois de o usuario escanear o servidor pede para
  reiniciar a conexao e a reconexao nunca fecha (portal de autenticacao que
  caiu depois do scan, porta 443 intermitente). Agora o relogio so e renovado
  quando o pareamento chega MAIS LONGE do que ja esteve, e como esse avanco e
  monotono o numero de renovacoes e finito: nenhuma fase, e nenhum ciclo de
  fases, escapa. Isso fecha tambem o caso de uma ponte que conecta, cai e
  conecta de novo sem parar, que renovava o relogio a cada volta. Um teste
  enumera todas as fases e nao compila se alguem acrescentar uma sem dizer
  qual e o teto dela.
- **O modo `serve` desiste quando a ponte fala sem nunca conectar (#1238).**
  E o mesmo problema do lado do daemon: o prazo do laco de eventos conta desde
  o ultimo evento, e uma ponte que reconecta sozinha o realimenta a cada
  tentativa fracassada. O canal ficava preso numa execucao que tentava para
  sempre e o backoff de reconexao nunca chegava a rodar — sem ninguem olhando
  um terminal. Agora ha um teto de tempo falando sem conectar, e estoura-lo
  entrega o caso ao backoff que ja existia.
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
  buffer do pipe enche e o filho para de ler. Agora ele tem 500 ms e desiste:
  o que se espera ali e uma linha curta num pipe saudavel, e quem nao
  respondeu nesse tempo nao vai responder.
- **A linha de "nova tentativa em Ns" deixou de prometer prazo ja vencido
  (#1238).** Ela truncava a divisao, entao um backoff de 1500 ms aparecia como
  "1s" e a linha ficava vencida meio segundo depois. Junto disso a faixa de
  500 a 999 ms mudou de aparencia: antes ela caia no generico "tentando de
  novo", e agora arredonda para cima e anuncia "1s".
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
