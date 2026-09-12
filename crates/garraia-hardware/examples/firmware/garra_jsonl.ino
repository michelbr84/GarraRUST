// garra_jsonl.ino — firmware de referência do adapter Serial/USB do GarraIA.
//
// Issue #1130 (epic #1124, ADR 0020). Fala o protocolo JSONL que o
// `crates/garraia-hardware/src/adapter_serial.rs` implementa do outro lado do
// cabo: uma mensagem por linha, terminada em '\n'.
//
//   gateway -> placa   {"garra_hello":true}
//   placa   -> gateway {"garra_hello":true,"id":"...","capabilities":[...]}
//   gateway -> placa   {"request_id":"req-1","op":"get","capability":"digital_read"}
//   placa   -> gateway {"request_id":"req-1","value":{"pins":{"2":1}}}
//   gateway -> placa   {"request_id":"req-2","op":"set","capability":"digital_write",
//                       "args":{"pin":13,"value":1}}
//   placa   -> gateway {"request_id":"req-2","value":{"pin":13,"value":1}}
//   placa   -> gateway {"error":"...","request_id":"req-2"}   (recusa)
//   placa   -> gateway {"state":{"pins":{"2":1}}}             (espontâneo)
//
// ---------------------------------------------------------------------------
// DUAS COISAS QUE VALEM SABER ANTES DE EDITAR
// ---------------------------------------------------------------------------
//
// 1. O gateway NÃO aceita risk class vindo daqui. O manifesto declara apenas
//    os NOMES das capabilities; quem diz que `digital_write` é R2 é a tabela
//    em `crates/garraia-hardware/src/perifericos.rs`, revisada no repo. Um
//    nome fora daquela tabela (digital_read, analog_read, digital_write, pwm)
//    faz o gateway recusar a placa inteira. Isso é de propósito: qualquer um
//    pluga um USB, então a placa não classifica o próprio risco.
//
// 2. A allowlist de pinos abaixo é a sua última linha de defesa física. O
//    gateway também valida, mas ele não sabe o que está soldado no seu
//    protoboard. Declare aqui SÓ os pinos que podem ser mexidos.
//
// ---------------------------------------------------------------------------
// COMO USAR
// ---------------------------------------------------------------------------
//
//   1. Abra este arquivo na Arduino IDE (ou use arduino-cli — ver README.md).
//   2. Ajuste PLACA_ID, PINOS_ENTRADA, PINOS_SAIDA e PINOS_ANALOGICOS.
//   3. Selecione a placa e a porta, e grave.
//   4. No `garraia.toml`, aponte a porta:
//
//        [hardware.serial]
//        portas = ["/dev/ttyACM0"]   # ou "COM3" no Windows
//        baud   = 115200
//
// Sem bibliotecas externas de propósito (nada de ArduinoJson): o protocolo é
// simples o bastante para caber num Uno com 2 KB de RAM, e uma dependência a
// menos é uma coisa a menos para dar errado na bancada de quem está começando.

// --------------------------------------------------------------------------
// Configuração — é aqui que você mexe.
// --------------------------------------------------------------------------

// Id estável desta placa. Vira a chave no registry do Garra, então precisa ser
// único entre as suas placas. Só ASCII alfanumérico, '-', '_' e '.' (o gateway
// recusa o resto), no máximo 64 caracteres.
const char PLACA_ID[] = "arduino-bancada";

const long BAUD = 115200;

// Pinos digitais lidos por `digital_read`.
const uint8_t PINOS_ENTRADA[] = {2, 3};
// Pinos digitais que `digital_write` e `pwm` podem mexer. TUDO que não estiver
// aqui é recusado — inclusive por engano do gateway.
const uint8_t PINOS_SAIDA[] = {12, 13};
// Canais lidos por `analog_read`. Deixe vazio ({}) numa placa sem ADC.
const uint8_t PINOS_ANALOGICOS[] = {A0};

// Manda `{"state":...}` sozinho quando uma entrada muda de nível. O gateway
// transforma isso em evento para o motor de automações (#1128). Coloque
// `false` para uma placa puramente sob demanda.
const bool AVISAR_MUDANCAS = true;
// Intervalo mínimo entre avisos espontâneos, em ms (debounce simples).
const unsigned long DEBOUNCE_MS = 50;

// Teto da linha recebida. O gateway corta em 64 KiB; aqui o limite é a RAM.
const size_t LINHA_MAX = 200;

// --------------------------------------------------------------------------
// Daqui para baixo, normalmente não é preciso mexer.
// --------------------------------------------------------------------------

const uint8_t N_ENTRADA = sizeof(PINOS_ENTRADA) / sizeof(PINOS_ENTRADA[0]);
const uint8_t N_SAIDA = sizeof(PINOS_SAIDA) / sizeof(PINOS_SAIDA[0]);
const uint8_t N_ANALOGICO = sizeof(PINOS_ANALOGICOS) / sizeof(PINOS_ANALOGICOS[0]);

char linha[LINHA_MAX + 1];
size_t usado = 0;
// Ligado quando a linha estourou o buffer: tudo até o próximo '\n' é lixo de
// uma mensagem que já não cabe, e processar a cauda dela seria processar meia
// mensagem (ou, pior, a segunda metade de uma mensagem forjada).
bool descartando = false;

int ultimoNivel[N_ENTRADA > 0 ? N_ENTRADA : 1];
unsigned long ultimoAviso = 0;

bool ehSaida(long pino) {
  for (uint8_t i = 0; i < N_SAIDA; i++) {
    if ((long)PINOS_SAIDA[i] == pino) return true;
  }
  return false;
}

// Extrai o texto de "chave":"valor". Devolve "" se não achar.
// Suficiente para este protocolo, que nunca manda aspas escapadas dentro de
// um valor de texto.
String campoTexto(const String& origem, const char* chave) {
  String alvo = String("\"") + chave + "\":\"";
  int inicio = origem.indexOf(alvo);
  if (inicio < 0) return String("");
  inicio += alvo.length();
  int fim = origem.indexOf('"', inicio);
  if (fim < 0) return String("");
  return origem.substring(inicio, fim);
}

// Extrai o número de "chave":<n>. Devolve `padrao` se não achar.
long campoNumero(const String& origem, const char* chave, long padrao) {
  String alvo = String("\"") + chave + "\":";
  int inicio = origem.indexOf(alvo);
  if (inicio < 0) return padrao;
  inicio += alvo.length();
  while (inicio < (int)origem.length() && origem[inicio] == ' ') inicio++;
  int fim = inicio;
  if (fim < (int)origem.length() && (origem[fim] == '-' || origem[fim] == '+')) fim++;
  while (fim < (int)origem.length() && isDigit(origem[fim])) fim++;
  if (fim == inicio) return padrao;
  return origem.substring(inicio, fim).toInt();
}

void responderErro(const String& rid, const char* motivo) {
  Serial.print(F("{\"request_id\":\""));
  Serial.print(rid);
  Serial.print(F("\",\"error\":\""));
  Serial.print(motivo);
  Serial.println(F("\"}"));
}

void enviarManifesto() {
  Serial.print(F("{\"garra_hello\":true,\"id\":\""));
  Serial.print(PLACA_ID);
  Serial.print(F("\",\"capabilities\":["));
  bool primeiro = true;
  if (N_ENTRADA > 0) {
    Serial.print(F("\"digital_read\""));
    primeiro = false;
  }
  if (N_ANALOGICO > 0) {
    if (!primeiro) Serial.print(',');
    Serial.print(F("\"analog_read\""));
    primeiro = false;
  }
  if (N_SAIDA > 0) {
    if (!primeiro) Serial.print(',');
    Serial.print(F("\"digital_write\",\"pwm\""));
  }
  Serial.println(F("]}"));
}

// Imprime {"2":1,"3":0} com os níveis das entradas.
void imprimirEntradas() {
  Serial.print('{');
  for (uint8_t i = 0; i < N_ENTRADA; i++) {
    if (i) Serial.print(',');
    Serial.print('"');
    Serial.print(PINOS_ENTRADA[i]);
    Serial.print(F("\":"));
    Serial.print(digitalRead(PINOS_ENTRADA[i]) == HIGH ? 1 : 0);
  }
  Serial.print('}');
}

void atenderGet(const String& rid, const String& capability) {
  if (capability == "digital_read") {
    Serial.print(F("{\"request_id\":\""));
    Serial.print(rid);
    Serial.print(F("\",\"value\":{\"pins\":"));
    imprimirEntradas();
    Serial.println(F("}}"));
    return;
  }
  if (capability == "analog_read") {
    Serial.print(F("{\"request_id\":\""));
    Serial.print(rid);
    Serial.print(F("\",\"value\":{\"pins\":{"));
    for (uint8_t i = 0; i < N_ANALOGICO; i++) {
      if (i) Serial.print(',');
      Serial.print('"');
      Serial.print(PINOS_ANALOGICOS[i]);
      Serial.print(F("\":"));
      Serial.print(analogRead(PINOS_ANALOGICOS[i]));
    }
    Serial.println(F("}}}"));
    return;
  }
  responderErro(rid, "capability desconhecida");
}

void atenderSet(const String& rid, const String& capability, const String& origem) {
  long pino = campoNumero(origem, "pin", -1);
  if (pino < 0) {
    responderErro(rid, "argumento pin ausente");
    return;
  }
  if (!ehSaida(pino)) {
    // A allowlist da placa. O gateway já barra o que não declaramos no
    // manifesto, mas ele não sabe o que está soldado aqui.
    responderErro(rid, "pino nao declarado como saida nesta placa");
    return;
  }

  if (capability == "digital_write") {
    long valor = campoNumero(origem, "value", -1);
    if (valor != 0 && valor != 1) {
      responderErro(rid, "value precisa ser 0 ou 1");
      return;
    }
    digitalWrite((uint8_t)pino, valor == 1 ? HIGH : LOW);
    Serial.print(F("{\"request_id\":\""));
    Serial.print(rid);
    Serial.print(F("\",\"value\":{\"pin\":"));
    Serial.print(pino);
    Serial.print(F(",\"value\":"));
    Serial.print(valor);
    Serial.println(F("}}"));
    return;
  }

  if (capability == "pwm") {
    long duty = campoNumero(origem, "duty", -1);
    if (duty < 0 || duty > 255) {
      responderErro(rid, "duty precisa estar entre 0 e 255");
      return;
    }
    analogWrite((uint8_t)pino, (int)duty);
    Serial.print(F("{\"request_id\":\""));
    Serial.print(rid);
    Serial.print(F("\",\"value\":{\"pin\":"));
    Serial.print(pino);
    Serial.print(F(",\"duty\":"));
    Serial.print(duty);
    Serial.println(F("}}"));
    return;
  }

  responderErro(rid, "capability desconhecida");
}

void processar(const String& origem) {
  // Handshake: o gateway abre toda conversa com ele, inclusive ao reconectar.
  if (origem.indexOf("\"garra_hello\"") >= 0) {
    enviarManifesto();
    return;
  }
  String rid = campoTexto(origem, "request_id");
  if (rid.length() == 0) return;  // sem correlação, não há o que responder
  String op = campoTexto(origem, "op");
  String capability = campoTexto(origem, "capability");

  if (op == "get") {
    atenderGet(rid, capability);
  } else if (op == "set") {
    atenderSet(rid, capability, origem);
  } else {
    responderErro(rid, "op desconhecida");
  }
}

void avisarMudancas() {
  if (!AVISAR_MUDANCAS || N_ENTRADA == 0) return;
  if (millis() - ultimoAviso < DEBOUNCE_MS) return;

  bool mudou = false;
  for (uint8_t i = 0; i < N_ENTRADA; i++) {
    int agora = digitalRead(PINOS_ENTRADA[i]);
    if (agora != ultimoNivel[i]) {
      ultimoNivel[i] = agora;
      mudou = true;
    }
  }
  if (!mudou) return;
  ultimoAviso = millis();
  Serial.print(F("{\"state\":{\"pins\":"));
  imprimirEntradas();
  Serial.println(F("}}"));
}

void setup() {
  Serial.begin(BAUD);
  for (uint8_t i = 0; i < N_ENTRADA; i++) {
    // INPUT_PULLUP é o default seguro para botão ligado ao GND. Troque por
    // INPUT se o seu circuito já tem resistor externo.
    pinMode(PINOS_ENTRADA[i], INPUT_PULLUP);
    ultimoNivel[i] = digitalRead(PINOS_ENTRADA[i]);
  }
  for (uint8_t i = 0; i < N_SAIDA; i++) {
    pinMode(PINOS_SAIDA[i], OUTPUT);
    // Estado inicial conhecido: nada de relé ligando sozinho no boot.
    digitalWrite(PINOS_SAIDA[i], LOW);
  }
  // Sem manifesto espontâneo aqui: o gateway pergunta primeiro. Mandar antes
  // do handshake só encheria o buffer de quem não está escutando.
}

void loop() {
  while (Serial.available() > 0) {
    char c = (char)Serial.read();
    if (c == '\n') {
      // Fim da linha: ou ela cabia e é processada, ou era a que estourou o
      // buffer e o descarte termina aqui.
      linha[usado] = '\0';
      if (usado > 0 && !descartando) processar(String(linha));
      usado = 0;
      descartando = false;
    } else if (c != '\r') {
      if (descartando) {
        // Cauda da linha estourada: consome sem guardar, até o '\n'.
      } else if (usado < LINHA_MAX) {
        linha[usado++] = c;
      } else {
        // Linha maior que o buffer: descarta ela inteira, incluindo o que
        // ainda vem, em vez de processar meia mensagem.
        usado = 0;
        descartando = true;
      }
    }
  }
  avisarMudancas();
}
