# Firmware de referência — adapter Serial/USB do GarraIA

`garra_jsonl.ino` é o sketch que faz uma placa Arduino ou ESP32 aparecer como
dispositivo no GarraIA. Ele fala o protocolo JSONL implementado em
[`../../src/adapter_serial.rs`](../../src/adapter_serial.rs) — issue
[#1130](https://github.com/michelbr84/GarraRUST/issues/1130), epic
[#1124](https://github.com/michelbr84/GarraRUST/issues/1124).

Não é um alvo Cargo: é código C++ para a toolchain do Arduino. Por isso o
`Cargo.toml` do crate traz `autoexamples = false` — sem isso o Cargo tentaria
compilar este diretório como um exemplo Rust.

## O protocolo em cinco linhas

Uma mensagem por linha, terminada em `\n`, cada uma um objeto JSON:

```text
gateway -> placa   {"garra_hello":true}
placa   -> gateway {"garra_hello":true,"id":"arduino-bancada","capabilities":["digital_read","digital_write"]}
gateway -> placa   {"request_id":"req-1","op":"get","capability":"digital_read"}
placa   -> gateway {"request_id":"req-1","value":{"pins":{"2":1,"3":0}}}
gateway -> placa   {"request_id":"req-2","op":"set","capability":"digital_write","args":{"pin":13,"value":1}}
placa   -> gateway {"request_id":"req-2","value":{"pin":13,"value":1}}
```

A placa também pode recusar (`{"request_id":"req-2","error":"..."}`) e avisar
mudanças sozinha (`{"state":{"pins":{"2":1}}}`), que viram eventos para o motor
de automações.

## Gravando

**Arduino IDE:** abra `garra_jsonl.ino`, selecione a placa e a porta, clique em
Upload. Nenhuma biblioteca precisa ser instalada.

**arduino-cli:**

```bash
# Uno (e clones com CH340)
arduino-cli compile --fqbn arduino:avr:uno .
arduino-cli upload  --fqbn arduino:avr:uno -p /dev/ttyACM0 .

# ESP32 DevKit
arduino-cli compile --fqbn esp32:esp32:esp32 .
arduino-cli upload  --fqbn esp32:esp32:esp32 -p /dev/ttyUSB0 .
```

O diretório precisa ter o mesmo nome do `.ino` para o `arduino-cli` aceitá-lo
como sketch. Se o seu não aceitar, copie o arquivo para
`garra_jsonl/garra_jsonl.ino` antes de compilar.

## Antes de gravar, ajuste quatro coisas

No topo do sketch:

| Constante | O que é |
|---|---|
| `PLACA_ID` | id único desta placa no Garra (vira a chave do registry) |
| `PINOS_ENTRADA` | pinos lidos por `digital_read` |
| `PINOS_SAIDA` | **a allowlist**: só estes pinos podem ser escritos |
| `PINOS_ANALOGICOS` | canais lidos por `analog_read` (vazio numa placa sem ADC) |

`PINOS_SAIDA` é a parte que merece atenção. O gateway valida o pino, mas ele
não sabe o que está soldado na sua bancada; a placa é quem sabe. Deixe de fora
tudo que não deve ser mexido.

## Ligando no Garra

```toml
[hardware.serial]
# Sem esta linha, o Garra tenta descobrir a placa por VID/PID. No Linux sem
# libudev a descoberta não enxerga VID/PID e não acha nada — declarar a porta
# é o caminho confiável.
portas = ["/dev/ttyACM0"]   # ou "COM3" no Windows
baud   = 115200
```

Para um caminho que não muda quando você repluga a placa, use o link estável:

```bash
ls -l /dev/serial/by-id/
# portas = ["/dev/serial/by-id/usb-Arduino__www.arduino.cc__0043_XXXX-if00"]
```

No Linux, o seu usuário precisa estar no grupo dono da porta (`dialout` na
maioria das distros, `uucp` no Arch):

```bash
sudo usermod -aG dialout "$USER"   # e reabrir a sessão
```

## Risco: o que a placa pode e o que ela não pode declarar

O manifesto declara **nomes** de capability. Ele **não** declara risk class — e
não tem onde declarar. Quem diz que `digital_read` é R0 e que `digital_write` e
`pwm` são R2 é a tabela fechada em
[`../../src/perifericos.rs`](../../src/perifericos.rs), revisada no repositório.

Isso é deliberado. O adapter MQTT aceita o risco declarado pelo dispositivo
porque um broker tem ACL; um cabo USB não tem nada. Se o risco viesse do
manifesto, bastaria plugar uma placa que se anuncia como
`{"name":"digital_write","risk":"r0","read_only":true}` para o gate aprovar
acionamentos físicos automaticamente. Por isso um nome fora da tabela
(`digital_read`, `analog_read`, `digital_write`, `pwm`) faz o gateway recusar a
placa **inteira**, não só a capability estranha.

Na prática: R2 significa que a escrita passa pela policy do modo com rate
limit — nunca em automático.

## Depurando

O monitor serial da IDE funciona: o protocolo é texto. Com a placa conectada e
o Garra parado, abra o monitor a 115200, mande

```text
{"garra_hello":true}
```

e a placa deve responder o manifesto na mesma hora. Se responder, o Garra
também vai adotá-la.

Duas coisas que costumam aparecer:

- **A placa reinicia quando o Garra abre a porta.** É o auto-reset por DTR do
  Uno/Nano, não um bug: o gateway espera a resposta do handshake por 5 s por
  padrão, o que cobre o boot. Se a sua placa demora mais, aumente o timeout no
  config.
- **Linhas de `Serial.println("debug")` no meio do protocolo.** O gateway
  ignora linhas que não são JSON do protocolo (vira log em nível debug), então
  não quebram nada — mas atrapalham a leitura no monitor.
