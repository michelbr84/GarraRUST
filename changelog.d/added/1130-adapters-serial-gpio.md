- Terceira onda de transportes do `garraia-hardware` (#1130, epic #1124): uma
  placa Arduino ou ESP32 ligada pelo cabo USB vira dispositivo do agente. O
  adapter serial abre a porta, manda `{"garra_hello":true}` e adota a placa
  que responder com o manifesto; dai em diante o protocolo e JSONL, uma
  mensagem JSON por linha, com `request_id` correlacionando cada leitura e
  cada escrita. Porta que nao responde ao handshake e fechada e esquecida —
  um modem ou um GPS nunca viram "dispositivo".
- Firmware de referencia publicado em
  `crates/garraia-hardware/examples/firmware/`: um sketch sem bibliotecas
  externas (cabe num Uno) mais um README com o protocolo, o passo a passo de
  gravacao e as permissoes de porta no Linux.
- Adapter GPIO para Raspberry Pi: os pinos do proprio host expostos como
  `digital_read`, `digital_write` e `pwm`, com o operador declarando quais
  pinos sao entrada e quais sao saida. O que nao foi declarado e negado —
  escrever num pino fora da lista e erro, nao escrita silenciosa. Num host que
  nao e Raspberry Pi o adapter falha limpo, com o remedio no texto do erro
  (grupo `gpio` e `/dev/gpiomem`), sem registrar dispositivo fantasma.
- Risco vem do codigo, nao da placa: os dois adapters compartilham uma tabela
  fechada de capabilities — leitura (`digital_read`, `analog_read`) em R0 e
  escrita fisica (`digital_write`, `pwm`) em R2. O manifesto declara apenas os
  NOMES; nome fora da tabela recusa o dispositivo inteiro. Diferente do MQTT,
  onde o broker tem ACL e o risco pode vir do manifesto, qualquer pessoa com
  acesso fisico pluga um USB — entao a placa nao classifica o proprio risco.
- Hardening da superficie de descoberta: caminho de porta passa por allowlist
  de forma (`/dev/...` ou `COM<n>`, sem `..`), a descoberta automatica so
  considera portas USB com VID/PID conhecido, o leitor corta linha acima de 64
  KiB sem `\n` em vez de crescer o buffer, e pino, nivel e duty sao validados
  em faixa fechada antes de qualquer byte sair pela porta.
- Features `hardware-serial` e `hardware-gpio` OFF por default, mesmo padrao
  do `mqtt` e do `home-assistant`: quem nao liga hardware fisico nao paga a
  arvore de deps (`tokio-serial`/`serialport` e `rppal`). Sem as features, o
  crate compila e testa exatamente como antes.
