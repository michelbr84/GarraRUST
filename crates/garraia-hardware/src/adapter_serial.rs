//! Adapter Serial/USB (#1130) — Arduino e ESP32 falando JSONL pela porta.
//!
//! O terceiro transporte do `garraia-hardware`, atrás da feature
//! `hardware-serial`. Onde o MQTT (#1126) precisa de broker e o Home
//! Assistant (#1127) precisa de hub, aqui o caminho é o cabo: uma placa
//! rodando o sketch de referência (`examples/firmware/`) vira um
//! [`crate::Device`] no registry assim que responde ao handshake.
//!
//! # O protocolo: uma linha, um JSON
//!
//! Cada mensagem é **um objeto JSON numa linha**, terminada por `\n` (JSONL).
//! Nada de framing binário, nada de checksum: a camada USB-CDC já entrega
//! ordenado e íntegro, e uma linha de texto é depurável com o monitor serial
//! da IDE do Arduino — o que importa quando o outro lado é um microcontrolador
//! que alguém está programando pela primeira vez.
//!
//! | Direção | Linha |
//! |---|---|
//! | gateway → placa | `{"garra_hello":true}` |
//! | placa → gateway | `{"garra_hello":true,"id":"...","capabilities":["digital_read",...]}` |
//! | gateway → placa | `{"request_id":"req-1","op":"get","capability":"digital_read"}` |
//! | gateway → placa | `{"request_id":"req-2","op":"set","capability":"digital_write","args":{"pin":13,"value":1}}` |
//! | placa → gateway | `{"request_id":"req-1","value":{...}}` |
//! | placa → gateway | `{"request_id":"req-2","error":"pino 13 nao e saida"}` |
//! | placa → gateway | `{"state":{...}}` (espontâneo, sem `request_id`) |
//!
//! # Contratos
//!
//! - **Handshake**: o gateway abre a porta, escreve `{"garra_hello":true}` e
//!   espera o manifesto por [`SerialAdapterConfig::timeout`]. Porta que não
//!   responde é **fechada e esquecida** — é assim que um modem, um GPS ou o
//!   console serial da própria máquina não viram "dispositivo".
//! - **Risco vem do código, não da placa**: o manifesto declara só os
//!   **nomes** das capabilities; o risk class sai da tabela fechada em
//!   [`crate::perifericos`] (leitura R0, escrita R2). Nome fora da tabela
//!   recusa o dispositivo inteiro. O porquê está no doc daquele módulo — em
//!   resumo, um broker MQTT tem ACL e um cabo USB não tem.
//! - **Correlação**: cada `get`/`set` leva um `request_id`; a resposta é
//!   casada por ele, com timeout. Diferente do MQTT (que confirma só a
//!   entrega ao broker), aqui o link é ponto a ponto e o `execute` **espera o
//!   ack da placa** — o valor devolvido é o que a placa disse ter feito.
//! - **Limite de linha**: uma placa com firmware quebrado pode despejar bytes
//!   sem `\n` para sempre. O leitor corta em [`LINHA_MAX`] e descarta o
//!   dispositivo, em vez de crescer o buffer até o OOM.
//! - **Superfície de descoberta**: abrir uma porta serial arbitrária é a
//!   parte perigosa deste adapter. A descoberta automática só considera
//!   portas que o SO reporta como **USB com VID/PID na allowlist**; portas
//!   declaradas à mão passam por [`validar_caminho_porta`], que aceita só
//!   caminhos tty-like (`/dev/tty*`, `/dev/cu.*`, `/dev/serial/by-id/...`,
//!   `/dev/serial/by-path/...`) — `/dev/mem`, `/dev/sda` e `/dev/watchdog`
//!   são `/dev/...` e nem por isso são portas seriais.
//! - **Id é do transporte, não da placa**: a placa escolhe o texto do `id` no
//!   manifesto, então esse texto **não** pode virar chave do registry
//!   diretamente — um firmware hostil se anunciaria com o id de um device do
//!   Home Assistant e passaria a receber as leituras e os comandos dele. Duas
//!   camadas resolvem: o id de registro é **namespaceado** com
//!   [`PREFIXO_ID`] (`serial:arduino-1`), e `validar_id` proíbe `:` no id
//!   declarado, então nenhuma placa alcança o namespace de outro transporte;
//!   e, dentro do próprio namespace serial, a adoção usa
//!   [`DeviceRegistry::register_if_absent`] e **recusa** (fail-closed) a
//!   segunda placa que chegar com um id já ocupado, em vez de substituí-la em
//!   silêncio. O id declarado segue disponível como campo informativo
//!   ([`SerialDevice::id_declarado`]).
//! - **Texto da placa é entrada hostil**: todo `String` que vem do outro lado
//!   do cabo — texto de erro, id ecoado em mensagem, nome de capability,
//!   `value` de leitura, `state` espontâneo — passa por
//!   `sanear_texto_da_placa` antes de virar log, mensagem de erro ou
//!   resultado de tool. Os detalhes (o que é filtrado, e por que o teto do
//!   `value` é [`VALOR_DA_PLACA_MAX`] e não [`LINHA_MAX`]) estão no doc
//!   daquelas funções.
//! - **Presença e barramento**: o registro marca online no
//!   [`DeviceStateStore`]; EOF ou erro de leitura marca offline. Os dois
//!   publicam no [`HardwareEventBus`], e linhas espontâneas com `state`
//!   viram `StateChanged` para o motor de automações (#1128).
//!
//! # Descoberta por VID/PID: o que funciona onde
//!
//! `serialport` entra aqui **sem** a feature `libudev` (ver o comentário no
//! `Cargo.toml`: ligá-la exigiria `libudev-dev` na máquina de build e
//! quebraria `cargo check --all-features` no CI). A consequência é concreta e
//! vale saber antes de depurar:
//!
//! - **macOS e Windows**: `available_ports()` reporta VID/PID e a descoberta
//!   automática funciona.
//! - **Linux sem udev**: as portas aparecem, mas sem VID/PID
//!   (`SerialPortType::Unknown`). A descoberta automática, que é fail-closed,
//!   **não acha nada** — e o operador declara a porta explicitamente
//!   (`/dev/ttyUSB0`, `/dev/ttyACM0`, ou o caminho estável em
//!   `/dev/serial/by-id/...`). Nenhuma porta é aberta "no chute".

use crate::Result;
use crate::capability::Capability;
use crate::device::Device;
use crate::error::HardwareError;
use crate::events::{EstadoObservado, HardwareEvent, HardwareEventBus, StateChanged};
use crate::perifericos;
use crate::registry::DeviceRegistry;
use crate::schema::validar_args;
use crate::state::DeviceStateStore;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Mutex, oneshot};

/// Baud rate padrão — o que os exemplos de Arduino e ESP32 usam.
pub const BAUD_DEFAULT: u32 = 115_200;

/// Teto de baud aceito na config. Acima disso é erro de digitação, não
/// configuração.
pub const BAUD_MAX: u32 = 4_000_000;

/// Timeout padrão do handshake e de cada `get`/`set`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Maior linha aceita da placa. 64 KiB é ordens de grandeza acima de
/// qualquer resposta honesta (um `digital_read` de 64 pinos não passa de
/// alguns KB) e pequeno o bastante para não virar vetor de memória.
pub const LINHA_MAX: usize = 64 * 1024;

/// Bytes lidos por vez da porta.
const CHUNK: usize = 1024;

/// VID/PID das pontes USB-serial que aparecem em placas Arduino e ESP32.
///
/// É uma **allowlist**, não uma lista de compatibilidade: a placa precisa
/// responder ao handshake de qualquer jeito. O papel dela é evitar que a
/// descoberta sequer *abra* um modem 4G ou um leitor de cartão que também se
/// apresenta como porta serial.
pub const VID_PID_CONHECIDOS: &[(u16, u16)] = &[
    (0x2341, 0x0043), // Arduino Uno R3
    (0x2341, 0x0001), // Arduino Uno R1
    (0x2341, 0x0243), // Arduino Uno R3 (clone oficial)
    (0x2A03, 0x0043), // Arduino (arduino.org)
    (0x1A86, 0x7523), // CH340 — clones de Uno/Nano e muito ESP32
    (0x1A86, 0x55D4), // CH9102
    (0x10C4, 0xEA60), // CP2102 — ESP32 DevKit
    (0x0403, 0x6001), // FTDI FT232
    (0x0403, 0x6015), // FTDI FT231X
    (0x303A, 0x1001), // Espressif USB-JTAG/serial nativo (ESP32-S2/S3/C3)
];

/// Mapa de requisições em voo: `request_id` → canal de resposta.
type Pendentes = Mutex<HashMap<String, oneshot::Sender<RespostaPlaca>>>;

/// A metade de escrita da porta, apagada para `dyn` — é o que deixa o mesmo
/// código servir a porta real e ao par de PTYs dos testes.
type Escrita = Box<dyn AsyncWrite + Send + Unpin>;

static SEQUENCIA: AtomicU64 = AtomicU64::new(0);

fn novo_request_id() -> String {
    format!("req-{}", SEQUENCIA.fetch_add(1, Ordering::Relaxed))
}

/// Teto de cada pedaço de texto que a placa manda de volta.
const ERRO_DA_PLACA_MAX: usize = 300;

/// Teto do JSON de uma resposta bem-sucedida (`value`) ou de um `state`
/// espontâneo, já saneado e re-serializado.
///
/// Não é [`LINHA_MAX`]: aquele é o teto do *transporte* (o que o leitor
/// aceita antes de declarar a placa quebrada), e 64 KiB de texto escolhido
/// pela placa entrando no histórico do modelo como resultado de tool é caro e
/// é vetor de prompt injection por volume. 4 KiB é ordem de grandeza acima de
/// qualquer leitura honesta desta tabela de periféricos — 64 pinos digitais
/// em `{"pins":{"0":1,...}}` dão ~600 bytes — e continua legível para um
/// humano depurando.
///
/// Os dois tetos se dividem o trabalho: cada string isolada é **truncada** em
/// [`ERRO_DA_PLACA_MAX`] com `…` (o conteúdo útil de uma mensagem costuma
/// estar no começo), e é o total do JSON já saneado que bate aqui — estrutura
/// inflada (milhares de chaves, arrays longos) não tem começo útil para
/// preservar. Acima do teto a leitura **falha**: meio JSON é pior que nenhum.
pub const VALOR_DA_PLACA_MAX: usize = 4 * 1024;

/// Higieniza um texto vindo da placa antes de ele virar log, mensagem de erro
/// ou resultado de tool.
///
/// Esse texto é **entrada não confiável que chega ao modelo**: quem escreveu
/// o firmware (ou quem plugou a placa) escolhe cada byte, e o resultado vai
/// para o histórico da conversa como resposta de tool. Duas defesas simples:
///
/// - caracteres perigosos viram espaço. Controle (`Cc`) cobre o `\n` que
///   forjaria o fim de uma mensagem e o `\x1b` que abriria sequência ANSI no
///   terminal de quem lê o log; a faixa bidi/invisível
///   (`U+200B`–`U+200F`, `U+202A`–`U+202E`) e os separadores `U+2028`/`U+2029`
///   cobrem o resto — um `RIGHT-TO-LEFT OVERRIDE` reordena visualmente o que
///   um humano lê na tela de aprovação, e `U+2028` é quebra de linha para
///   quase todo parser de JS/log sem ser `Cc`;
/// - o comprimento é cortado em [`ERRO_DA_PLACA_MAX`], para uma placa
///   verborrágica não gastar o contexto do turno.
fn sanear_texto_da_placa(bruto: &str) -> String {
    let perigoso = |c: char| {
        c.is_control()
            || matches!(c, '\u{2028}' | '\u{2029}')
            || ('\u{200b}'..='\u{200f}').contains(&c)
            || ('\u{202a}'..='\u{202e}').contains(&c)
    };
    let mut saneado: String = bruto
        .chars()
        .take(ERRO_DA_PLACA_MAX)
        .map(|c| if perigoso(c) { ' ' } else { c })
        .collect();
    if bruto.chars().count() > ERRO_DA_PLACA_MAX {
        saneado.push('…');
    }
    saneado
}

/// Higieniza recursivamente um `Value` vindo da placa: toda string — valor
/// **e chave de objeto** — passa por [`sanear_texto_da_placa`].
///
/// A recursão é limitada porque a profundidade já é: `serde_json` recusa
/// aninhamento acima do próprio limite de recursão ao *parsear* a linha, então
/// nenhum `Value` que chega aqui é fundo o bastante para estourar a pilha.
fn sanear_valor(bruto: Value) -> Value {
    match bruto {
        Value::String(texto) => Value::String(sanear_texto_da_placa(&texto)),
        Value::Array(itens) => Value::Array(itens.into_iter().map(sanear_valor).collect()),
        Value::Object(mapa) => Value::Object(
            mapa.into_iter()
                .map(|(chave, valor)| (sanear_texto_da_placa(&chave), sanear_valor(valor)))
                .collect(),
        ),
        escalar => escalar,
    }
}

/// O `value`/`state` da placa, saneado e com teto de [`VALOR_DA_PLACA_MAX`].
///
/// `Err` quando o JSON saneado passa do teto — o chamador trata como resposta
/// inválida (leitura falha, `state` espontâneo descartado), nunca truncando.
fn sanear_valor_da_placa(bruto: Value, dispositivo: &str) -> Result<Value> {
    let saneado = sanear_valor(bruto);
    let tamanho = saneado.to_string().len();
    if tamanho > VALOR_DA_PLACA_MAX {
        return Err(HardwareError::Adapter {
            dispositivo: dispositivo.to_string(),
            fonte: format!(
                "resposta de {tamanho} bytes acima do teto de {VALOR_DA_PLACA_MAX} \
                 — descartada (fail-closed)"
            ),
        });
    }
    Ok(saneado)
}

// ─────────────────────────────────────────────────────────────────────────────
// Config.
// ─────────────────────────────────────────────────────────────────────────────

/// Config do adapter serial.
///
/// `garraia-hardware` não depende de `garraia-config`: o gateway lê
/// `hardware.serial` e monta esta struct. Sem nada aqui, `portas` vazio
/// significa "descubra por VID/PID".
#[derive(Debug, Clone)]
pub struct SerialAdapterConfig {
    /// Portas declaradas explicitamente. Vazio → descoberta por VID/PID.
    pub portas: Vec<String>,
    /// Baud rate (default [`BAUD_DEFAULT`]).
    pub baud: u32,
    /// Allowlist de VID/PID da descoberta (default [`VID_PID_CONHECIDOS`]).
    pub vid_pid: Vec<(u16, u16)>,
    /// Timeout do handshake e de cada requisição.
    pub timeout: Duration,
}

impl Default for SerialAdapterConfig {
    fn default() -> Self {
        Self {
            portas: Vec::new(),
            baud: BAUD_DEFAULT,
            vid_pid: VID_PID_CONHECIDOS.to_vec(),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl SerialAdapterConfig {
    /// Config com os defaults (descoberta por VID/PID, 115200 8N1).
    pub fn nova() -> Self {
        Self::default()
    }

    /// Declara uma porta explícita, validando o caminho.
    pub fn com_porta(mut self, porta: impl Into<String>) -> Result<Self> {
        let porta = porta.into();
        validar_caminho_porta(&porta)?;
        self.portas.push(porta);
        Ok(self)
    }

    /// Seta o baud rate.
    pub fn com_baud(mut self, baud: u32) -> Result<Self> {
        if baud == 0 || baud > BAUD_MAX {
            return Err(HardwareError::Serial(format!(
                "baud {baud} inválido: esperado 1..={BAUD_MAX}"
            )));
        }
        self.baud = baud;
        Ok(self)
    }

    /// Substitui a allowlist de VID/PID da descoberta.
    #[must_use]
    pub fn com_vid_pid(mut self, vid_pid: Vec<(u16, u16)>) -> Self {
        self.vid_pid = vid_pid;
        self
    }

    /// Seta o timeout de handshake/requisição.
    #[must_use]
    pub fn com_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Revalida a config inteira — o `spawn` chama antes de abrir qualquer
    /// porta.
    pub fn validar(&self) -> Result<()> {
        if self.baud == 0 || self.baud > BAUD_MAX {
            return Err(HardwareError::Serial(format!(
                "baud {} inválido: esperado 1..={BAUD_MAX}",
                self.baud
            )));
        }
        if self.timeout.is_zero() {
            return Err(HardwareError::Serial(
                "timeout zero: nenhuma resposta jamais chegaria a tempo".to_string(),
            ));
        }
        for porta in &self.portas {
            validar_caminho_porta(porta)?;
        }
        Ok(())
    }
}

/// Os prefixos de `/dev/` que são porta serial, e o que vem depois deles é um
/// nome só (sem `/`).
///
/// `/dev/tty*` cobre Linux (`ttyUSB0`, `ttyACM0`, `ttyS0`) e o lado
/// "callin" do macOS (`tty.usbserial-1`); `/dev/cu.*` é o lado "callout" do
/// macOS, que é o que se usa para falar com uma placa; `serial/by-id/` e
/// `serial/by-path/` são os links estáveis do udev, recomendados no README do
/// firmware justamente porque não mudam ao repluga.
const PREFIXOS_UNIX: [&str; 4] = ["tty", "cu.", "serial/by-id/", "serial/by-path/"];

/// Valida um caminho de porta antes de abri-lo.
///
/// Abrir um caminho arbitrário vindo de config é a superfície perigosa deste
/// adapter: um valor como `/etc/shadow` ou `../../dev/mem` não deve nem
/// chegar ao `open`. A regra é uma allowlist de forma, sem `..` em nenhum
/// segmento e sem bytes de controle:
///
/// - Unix: um dos [`PREFIXOS_UNIX`] sob `/dev/`, com nome não-vazio.
///   `/dev/<qualquer coisa>` **não** basta — `/dev/mem` (memória física),
///   `/dev/sda` (disco) e `/dev/watchdog` (reboot ao fechar o fd) moram em
///   `/dev/` e nenhum deles é uma placa;
/// - Windows: `COM<n>` ou `\\.\COM<n>`.
pub fn validar_caminho_porta(porta: &str) -> Result<()> {
    let recusa = |motivo: &str| {
        Err(HardwareError::Serial(format!(
            "porta '{porta}' recusada: {motivo}"
        )))
    };
    if porta.is_empty() {
        return recusa("caminho vazio");
    }
    if porta.chars().any(|c| c.is_control()) {
        return recusa("caminho contém caractere de controle");
    }
    if porta.split(['/', '\\']).any(|seg| seg == "..") {
        return recusa("caminho contém '..' (travessia de diretório)");
    }
    let janela = porta.strip_prefix(r"\\.\").unwrap_or(porta);
    let e_windows = janela
        .strip_prefix("COM")
        .is_some_and(|resto| !resto.is_empty() && resto.chars().all(|c| c.is_ascii_digit()));
    let e_unix = porta.strip_prefix("/dev/").is_some_and(|resto| {
        PREFIXOS_UNIX.iter().any(|prefixo| {
            resto
                .strip_prefix(prefixo)
                .is_some_and(|nome| !nome.is_empty() && !nome.contains('/'))
        })
    });
    if !e_windows && !e_unix {
        return recusa(
            "esperado '/dev/tty*', '/dev/cu.*', '/dev/serial/by-id/<nome>' ou \
             '/dev/serial/by-path/<nome>' (Unix), ou 'COM<n>' / '\\\\.\\COM<n>' (Windows)",
        );
    }
    Ok(())
}

/// Prefixo do namespace deste transporte no registry.
///
/// A chave de registro de uma placa é `serial:<id declarado>`. O `:` é o que
/// fecha o namespace: `validar_id` não o aceita no id declarado, então uma
/// placa não consegue escrever `serial:` (nem o prefixo de nenhum outro
/// transporte) dentro do próprio id para escapar dele.
pub const PREFIXO_ID: &str = "serial:";

/// A chave de registry de uma placa, a partir do id que ela declarou.
fn id_de_registro(declarado: &str) -> String {
    format!("{PREFIXO_ID}{declarado}")
}

/// Valida o id que a placa declarou — ele vira chave do registry (sob
/// [`PREFIXO_ID`]) e aparece em log e no inventário do agente, então nada de
/// espaço, barra, `:` ou caractere de controle.
///
/// O texto ecoado na mensagem de erro já vem saneado: um id recusado é, por
/// definição, texto arbitrário de uma placa que não segue o contrato.
fn validar_id(id: &str) -> Result<()> {
    let eco = sanear_texto_da_placa(id);
    if id.is_empty() || id.len() > 64 {
        return Err(HardwareError::Serial(format!(
            "id '{eco}' inválido: esperado 1..=64 caracteres"
        )));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(HardwareError::Serial(format!(
            "id '{eco}' inválido: só ASCII alfanumérico, '-', '_' e '.'"
        )));
    }
    Ok(())
}

/// As portas que a descoberta considera: USB, com VID/PID na allowlist.
///
/// Fail-closed — porta sem VID/PID reportado (Linux sem udev) não entra.
pub fn portas_descobertas(vid_pid: &[(u16, u16)]) -> Vec<String> {
    let portas = match serialport::available_ports() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "serial: enumeração de portas falhou");
            return Vec::new();
        }
    };
    portas
        .into_iter()
        .filter_map(|porta| match &porta.port_type {
            serialport::SerialPortType::UsbPort(info)
                if vid_pid
                    .iter()
                    .any(|(v, p)| *v == info.vid && *p == info.pid) =>
            {
                Some(porta.port_name)
            }
            _ => None,
        })
        .filter(|nome| validar_caminho_porta(nome).is_ok())
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Protocolo — as linhas.
// ─────────────────────────────────────────────────────────────────────────────

/// O manifesto que a placa devolve ao handshake.
///
/// Só **nomes** de capability: o risco sai da tabela de
/// [`crate::perifericos`], não daqui.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestoSerial {
    /// Marcador do protocolo — tem que ser `true`.
    pub garra_hello: bool,
    /// Id estável da placa.
    pub id: String,
    /// Nomes das capabilities expostas, da tabela de periféricos.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Uma linha de resposta correlacionada.
#[derive(Debug, Clone, Deserialize)]
struct RespostaBruta {
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    value: Value,
    #[serde(default)]
    error: Option<String>,
    /// Estado espontâneo (sem `request_id`) — vira `StateChanged`.
    #[serde(default)]
    state: Option<Value>,
}

/// O que o `read`/`execute` recebe de volta pelo canal.
#[derive(Debug, Clone)]
enum RespostaPlaca {
    Valor(Value),
    Erro(String),
}

/// Parseia o manifesto do handshake, validando id e capabilities na
/// fronteira. Falha fechada: qualquer problema recusa a placa inteira.
///
/// Devolve o id **declarado** (já validado); a chave de registro sai dele por
/// [`id_de_registro`].
fn parse_manifesto(linha: &str) -> Result<(String, Vec<Capability>)> {
    let manifesto: ManifestoSerial = serde_json::from_str(linha).map_err(|e| {
        // O erro do serde pode citar o valor recebido ("invalid type: string
        // \"...\""), então ele carrega texto da placa e é saneado como tal.
        HardwareError::Serial(format!(
            "manifesto não é JSON válido: {}",
            sanear_texto_da_placa(&e.to_string())
        ))
    })?;
    if !manifesto.garra_hello {
        return Err(HardwareError::Serial(
            "manifesto sem 'garra_hello': true".to_string(),
        ));
    }
    validar_id(&manifesto.id)?;
    // Os nomes de capability também são texto da placa e são ecoados pelo
    // `resolver` na mensagem de recusa. Sanear antes não muda o resultado de
    // um nome legítimo (a tabela é ASCII fechada) e tira o texto hostil do
    // caminho do erro.
    let nomes: Vec<String> = manifesto
        .capabilities
        .iter()
        .map(|nome| sanear_texto_da_placa(nome))
        .collect();
    let caps = perifericos::resolver(&nomes, &manifesto.id)?;
    Ok((manifesto.id, caps))
}

// ─────────────────────────────────────────────────────────────────────────────
// Leitor de linhas com teto.
// ─────────────────────────────────────────────────────────────────────────────

/// Lê linhas JSONL de um `AsyncRead`, com teto de [`LINHA_MAX`].
///
/// Não é `BufReader::lines()` de propósito: aquele cresce o buffer sem limite
/// e uma placa que nunca manda `\n` derrubaria o processo por memória.
struct LeitorLinhas<R> {
    inner: R,
    pendente: Vec<u8>,
}

impl<R: AsyncRead + Unpin> LeitorLinhas<R> {
    fn novo(inner: R) -> Self {
        Self {
            inner,
            pendente: Vec::with_capacity(CHUNK),
        }
    }

    /// A próxima linha não-vazia, ou `None` no EOF.
    async fn proxima(&mut self) -> Result<Option<String>> {
        loop {
            if let Some(pos) = self.pendente.iter().position(|b| *b == b'\n') {
                let linha: Vec<u8> = self.pendente.drain(..=pos).collect();
                let texto = String::from_utf8_lossy(&linha).trim().to_string();
                if texto.is_empty() {
                    // Placa mandando `\r\n` sozinho ou linha em branco entre
                    // mensagens — segue lendo em vez de tratar como protocolo.
                    continue;
                }
                return Ok(Some(texto));
            }
            if self.pendente.len() > LINHA_MAX {
                return Err(HardwareError::Serial(format!(
                    "linha acima de {LINHA_MAX} bytes sem '\\n' — dispositivo descartado (fail-closed)"
                )));
            }
            let mut chunk = [0u8; CHUNK];
            let lidos = self
                .inner
                .read(&mut chunk)
                .await
                .map_err(|e| HardwareError::Serial(format!("leitura da porta: {e}")))?;
            if lidos == 0 {
                return Ok(None);
            }
            self.pendente.extend_from_slice(&chunk[..lidos]);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SerialDevice.
// ─────────────────────────────────────────────────────────────────────────────

/// Uma placa conectada por serial, exposta como [`Device`].
pub struct SerialDevice {
    /// A chave do registry: `serial:<id declarado>` (ver [`PREFIXO_ID`]).
    id: String,
    /// O id cru do manifesto, **informativo** — é o que está gravado no
    /// sketch e o que o operador vê no monitor serial. Não endereça nada.
    declarado: String,
    caps: Vec<Capability>,
    escrita: Arc<Mutex<Escrita>>,
    pendentes: Arc<Pendentes>,
    timeout: Duration,
    /// A porta (ou rótulo do stream, nos testes) — só para mensagem de erro.
    rotulo: String,
}

impl SerialDevice {
    /// O id que a placa declarou no manifesto, sem o [`PREFIXO_ID`].
    ///
    /// Informativo: serve para o operador casar o device do inventário com o
    /// `PLACA_ID` gravado no sketch. Quem endereça é [`Device::id`].
    pub fn id_declarado(&self) -> &str {
        &self.declarado
    }

    fn capability(&self, nome: &str) -> Result<&Capability> {
        self.caps.iter().find(|c| c.name == nome).ok_or_else(|| {
            HardwareError::CapabilityDesconhecida {
                dispositivo: self.id.clone(),
                capability: nome.to_string(),
            }
        })
    }

    fn erro(&self, fonte: String) -> HardwareError {
        HardwareError::Adapter {
            dispositivo: self.id.clone(),
            fonte,
        }
    }

    /// Manda uma requisição e espera a resposta correlacionada.
    async fn requisitar(&self, op: &str, capability: &str, args: Option<Value>) -> Result<Value> {
        let rid = novo_request_id();
        let (tx, rx) = oneshot::channel();
        self.pendentes.lock().await.insert(rid.clone(), tx);

        let mut linha = json!({
            "request_id": rid,
            "op": op,
            "capability": capability,
        });
        if let Some(args) = args {
            linha["args"] = args;
        }
        let mut bytes = linha.to_string().into_bytes();
        bytes.push(b'\n');

        let enviado = {
            let mut escrita = self.escrita.lock().await;
            match escrita.write_all(&bytes).await {
                Ok(()) => escrita.flush().await,
                Err(e) => Err(e),
            }
        };
        if let Err(e) = enviado {
            self.pendentes.lock().await.remove(&rid);
            return Err(self.erro(format!("falha ao escrever em '{}': {e}", self.rotulo)));
        }

        match tokio::time::timeout(self.timeout, rx).await {
            // O `value` de sucesso é tão escolhido pela placa quanto o texto
            // de erro: ele vira resultado de tool no histórico do modelo.
            Ok(Ok(RespostaPlaca::Valor(valor))) => sanear_valor_da_placa(valor, &self.id),
            // A placa respondeu, e respondeu "não deu" — o texto dela é o que
            // o modelo precisa ler, depois de higienizado.
            Ok(Ok(RespostaPlaca::Erro(msg))) => Err(self.erro(format!(
                "a placa recusou '{op} {capability}': {}",
                sanear_texto_da_placa(&msg)
            ))),
            Ok(Err(_)) => Err(self.erro(
                "porta encerrada antes da resposta (leitor caiu — dispositivo offline)".to_string(),
            )),
            Err(_) => {
                self.pendentes.lock().await.remove(&rid);
                Err(self.erro(format!(
                    "sem resposta em {:?} para '{op} {capability}' em '{}'",
                    self.timeout, self.rotulo
                )))
            }
        }
    }
}

#[async_trait]
impl Device for SerialDevice {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.caps.clone()
    }

    async fn read(&self, capability: &str) -> Result<Value> {
        let cap = self.capability(capability)?;
        // Simétrico ao `execute`, e pelo mesmo motivo invertido: `read` é o
        // caminho que o gate trata como R0. Ler por ele uma capability de
        // escrita seria pedir `digital_write` sem passar pela policy — o
        // `get` do protocolo não escreve, mas defesa em profundidade não
        // depende de o firmware do outro lado ser honesto.
        if !cap.read_only {
            return Err(self.erro(format!(
                "capability '{capability}' não é leitura: use execute, não read"
            )));
        }
        self.requisitar("get", capability, None).await
    }

    async fn execute(&self, capability: &str, args: Value) -> Result<Value> {
        let cap = self.capability(capability)?;
        // Uma capability read_only não tem caminho de escrita — executar uma
        // seria pedir ao gate R0 (`Auto`) uma ação no mundo. Fail-closed
        // antes de qualquer byte sair pela porta.
        if cap.read_only {
            return Err(self.erro(format!(
                "capability '{capability}' é read_only (R0): use read, não execute"
            )));
        }
        validar_args(&args, cap.args_schema.as_ref(), &self.id, capability)?;
        // Faixa fechada além do schema truncado (que não sabe expressar
        // intervalos): pino e valor entram na tabela de periféricos.
        perifericos::extrair_pino(&args, &self.id)?;
        match capability {
            perifericos::DIGITAL_WRITE => {
                perifericos::extrair_nivel(&args, &self.id)?;
            }
            perifericos::PWM => {
                perifericos::extrair_duty(&args, &self.id)?;
            }
            _ => {}
        }
        self.requisitar("set", capability, Some(args)).await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Adoção de um stream: handshake → registro → leitor.
// ─────────────────────────────────────────────────────────────────────────────

/// Faz o handshake num stream já aberto, registra a placa e sobe o leitor.
///
/// É o coração do adapter, e é genérico no stream de propósito: o
/// [`SerialAdapterManager`] passa um `tokio_serial::SerialStream` de uma porta
/// real, e os testes passam uma ponta de um par de PTYs
/// (`SerialStream::pair()`) — o mesmo código, sem hardware e sem docker.
///
/// Devolve a **chave de registro** (`serial:<id declarado>`, ver
/// [`PREFIXO_ID`]). Erro aqui significa "esta porta não é uma placa Garra" (ou
/// "é uma placa que não pode ser adotada") e o chamador simplesmente segue
/// para a próxima; o stream é devolvido ao nada (fechado no drop).
///
/// Uma chave já ocupada **recusa** a adoção, sem substituir o que estava lá.
/// Isso vale inclusive para a mesma placa reconectando: enquanto o device
/// antigo estiver no registry (marcado offline pelo leitor que caiu), a
/// re-adoção é recusada. Hoje isso não acontece, porque a varredura é uma
/// passada única no boot; quando hot-plug entrar, o desregistro no EOF é
/// pré-requisito dele.
pub async fn adotar_stream<S>(
    stream: S,
    rotulo: impl Into<String>,
    timeout: Duration,
    registry: Arc<DeviceRegistry>,
    state: Option<Arc<DeviceStateStore>>,
    bus: Option<Arc<HardwareEventBus>>,
) -> Result<String>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let rotulo = rotulo.into();
    let (leitura, escrita) = tokio::io::split(stream);
    let mut leitor = LeitorLinhas::novo(leitura);
    let escrita: Arc<Mutex<Escrita>> = Arc::new(Mutex::new(Box::new(escrita)));

    // 1. Handshake.
    {
        let mut guarda = escrita.lock().await;
        guarda
            .write_all(b"{\"garra_hello\":true}\n")
            .await
            .map_err(|e| HardwareError::Serial(format!("handshake em '{rotulo}': {e}")))?;
        guarda
            .flush()
            .await
            .map_err(|e| HardwareError::Serial(format!("handshake em '{rotulo}': {e}")))?;
    }

    let linha = tokio::time::timeout(timeout, leitor.proxima())
        .await
        .map_err(|_| {
            HardwareError::Serial(format!(
                "'{rotulo}' não respondeu ao handshake em {timeout:?} — não é uma placa Garra"
            ))
        })?
        .map_err(|e| HardwareError::Serial(format!("'{rotulo}': {e}")))?
        .ok_or_else(|| {
            HardwareError::Serial(format!("'{rotulo}' fechou a porta durante o handshake"))
        })?;

    let (declarado, caps) =
        parse_manifesto(&linha).map_err(|e| HardwareError::Serial(format!("'{rotulo}': {e}")))?;
    let id = id_de_registro(&declarado);

    // 2. Registro — fail-closed na colisão de id.
    let pendentes: Arc<Pendentes> = Arc::default();
    let device = SerialDevice {
        id: id.clone(),
        declarado,
        caps,
        escrita,
        pendentes: pendentes.clone(),
        timeout,
        rotulo: rotulo.clone(),
    };
    if !registry.register_if_absent(Arc::new(device)) {
        // Uma placa se anunciando com um id já registrado não substitui o
        // device de ninguém: ou é firmware duplicado na bancada, ou é alguém
        // plugando uma placa para sequestrar as leituras da outra. Os dois
        // casos se resolvem no operador, não em silêncio.
        tracing::warn!(
            dispositivo = %id,
            porta = %rotulo,
            "serial: id já registrado — adoção recusada (fail-closed)"
        );
        return Err(HardwareError::Serial(format!(
            "'{rotulo}': id '{id}' já está registrado — adoção recusada (fail-closed); \
             duas placas não podem dividir o mesmo id"
        )));
    }
    marcar(&id, true, state.as_ref(), bus.as_ref()).await;
    tracing::info!(dispositivo = %id, porta = %rotulo, "serial: placa descoberta e registrada");

    // 3. Leitor — vive enquanto a porta viver.
    tokio::spawn(rodar_leitor(leitor, id.clone(), pendentes, state, bus));
    Ok(id)
}

async fn rodar_leitor<R>(
    mut leitor: LeitorLinhas<R>,
    id: String,
    pendentes: Arc<Pendentes>,
    state: Option<Arc<DeviceStateStore>>,
    bus: Option<Arc<HardwareEventBus>>,
) where
    R: AsyncRead + Unpin,
{
    loop {
        match leitor.proxima().await {
            Ok(Some(linha)) => processar_linha(&id, &linha, &pendentes, bus.as_ref()).await,
            Ok(None) => {
                tracing::info!(dispositivo = %id, "serial: porta fechada (EOF) — offline");
                break;
            }
            Err(e) => {
                tracing::warn!(dispositivo = %id, error = %e, "serial: leitura falhou — offline");
                break;
            }
        }
    }
    // Quem esperava resposta recebe o canal fechado e vira erro de adapter;
    // o mapa é limpo para não segurar remetentes órfãos.
    pendentes.lock().await.clear();
    marcar(&id, false, state.as_ref(), bus.as_ref()).await;
}

async fn processar_linha(
    id: &str,
    linha: &str,
    pendentes: &Arc<Pendentes>,
    bus: Option<&Arc<HardwareEventBus>>,
) {
    let Ok(bruta) = serde_json::from_str::<RespostaBruta>(linha) else {
        // Sketch imprimindo debug com `Serial.println("...")` é o caso comum.
        tracing::debug!(dispositivo = %id, "serial: linha fora do protocolo, ignorada");
        return;
    };
    if let Some(rid) = bruta.request_id {
        let resposta = match bruta.error {
            Some(msg) => RespostaPlaca::Erro(msg),
            None => RespostaPlaca::Valor(bruta.value),
        };
        if let Some(tx) = pendentes.lock().await.remove(&rid) {
            let _ = tx.send(resposta);
        } else {
            tracing::debug!(dispositivo = %id, "serial: resposta sem requisição em voo — ignorada");
        }
        return;
    }
    // Linha espontânea com estado: alimenta o motor de automações (#1128) —
    // e chega ao modelo pelo mesmo caminho de um `value`, então é saneada
    // igual. Acima do teto, a linha é descartada, não truncada.
    if let Some(estado) = bruta.state
        && let Some(bus) = bus
    {
        let estado = match sanear_valor_da_placa(estado, id) {
            Ok(estado) => estado,
            Err(e) => {
                tracing::warn!(dispositivo = %id, error = %e, "serial: estado espontâneo descartado");
                return;
            }
        };
        bus.publicar(HardwareEvent::StateChanged(StateChanged::agora(
            id,
            EstadoObservado::com(None, estado, true),
            None,
        )));
    }
}

/// Marca presença no store e publica no barramento — os dois são opcionais.
async fn marcar(
    id: &str,
    online: bool,
    state: Option<&Arc<DeviceStateStore>>,
    bus: Option<&Arc<HardwareEventBus>>,
) {
    if let Some(store) = state
        && let Err(e) = store.marcar(id, online).await
    {
        tracing::warn!(dispositivo = %id, error = %e, "serial: falha ao marcar presença");
    }
    if let Some(bus) = bus {
        bus.publicar(HardwareEvent::StateChanged(StateChanged::agora(
            id,
            EstadoObservado::com(
                Some(if online { "online" } else { "offline" }.into()),
                Value::Null,
                online,
            ),
            None,
        )));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Manager.
// ─────────────────────────────────────────────────────────────────────────────

/// Boot do adapter serial: resolve as portas, abre cada uma e adota o que
/// responder ao handshake.
pub struct SerialAdapterManager {
    tarefa: tokio::task::JoinHandle<()>,
}

impl SerialAdapterManager {
    /// Sobe a varredura numa task tokio. Chamar dentro de um runtime.
    ///
    /// A varredura é **uma passada**: as portas presentes no boot são
    /// tentadas e as que responderem viram dispositivos. Hot-plug (uma placa
    /// conectada depois) fica para um slice próprio — o `udev`/`WM_DEVICECHANGE`
    /// que ele exige é trabalho de plataforma, não deste PR.
    pub fn spawn(
        config: SerialAdapterConfig,
        registry: Arc<DeviceRegistry>,
        state: Option<Arc<DeviceStateStore>>,
        bus: Option<Arc<HardwareEventBus>>,
    ) -> Result<Self> {
        config.validar()?;
        let tarefa = tokio::spawn(async move {
            let portas = if config.portas.is_empty() {
                let achadas = portas_descobertas(&config.vid_pid);
                if achadas.is_empty() {
                    tracing::info!(
                        "serial: descoberta não achou porta USB com VID/PID conhecido \
                         (no Linux sem udev isso é esperado — declare a porta no config)"
                    );
                }
                achadas
            } else {
                config.portas.clone()
            };
            for porta in portas {
                let builder = tokio_serial::new(&porta, config.baud);
                let stream = match tokio_serial::SerialStream::open(&builder) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(porta = %porta, error = %e, "serial: não abriu a porta");
                        continue;
                    }
                };
                match adotar_stream(
                    stream,
                    porta.clone(),
                    config.timeout,
                    registry.clone(),
                    state.clone(),
                    bus.clone(),
                )
                .await
                {
                    Ok(id) => tracing::info!(porta = %porta, dispositivo = %id, "serial: adotada"),
                    Err(e) => {
                        tracing::info!(porta = %porta, motivo = %e, "serial: porta ignorada")
                    }
                }
            }
        });
        Ok(Self { tarefa })
    }

    /// Para a varredura (shutdown, reload de config).
    ///
    /// Os dispositivos já adotados seguem vivos: quem os mantém é a task de
    /// leitura de cada porta, e ela morre quando a porta fecha.
    pub async fn encerrar(self) {
        self.tarefa.abort();
        let _ = self.tarefa.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Caminho de porta ──────────────────────────────────────────────────

    /// A allowlist de forma é o que separa "abrir uma placa" de "abrir um
    /// arquivo qualquer que o config apontar".
    #[test]
    fn caminho_de_porta_so_aceita_forma_conhecida() {
        for ok in [
            "/dev/ttyUSB0",
            "/dev/ttyACM0",
            "/dev/ttyS0",
            "/dev/tty.usbserial-1",
            "/dev/cu.usbserial-1",
            "/dev/cu.usbmodem14201",
            "/dev/serial/by-id/usb-Arduino-if00",
            "/dev/serial/by-path/pci-0000:00:14.0-usb-0:1:1.0-port0",
            "COM3",
            "COM17",
            r"\\.\COM9",
        ] {
            validar_caminho_porta(ok).unwrap_or_else(|e| panic!("'{ok}' deveria passar: {e}"));
        }
        for ruim in [
            "",
            "/etc/shadow",
            "/dev/",
            "../dev/ttyUSB0",
            "/dev/../etc/shadow",
            "ttyUSB0",
            "COM",
            "COMX",
            "/dev/tty\nUSB0",
        ] {
            assert!(
                validar_caminho_porta(ruim).is_err(),
                "'{ruim}' deveria ser recusado"
            );
        }
    }

    /// Estar em `/dev/` não faz de um arquivo uma porta serial. Estes três
    /// passavam pela allowlist "qualquer `/dev/<algo>`" e são exatamente os
    /// que não deveriam: memória física, disco e o watchdog (que reinicia a
    /// máquina quando o fd fecha).
    #[test]
    fn dispositivo_de_dev_que_nao_e_serial_e_recusado() {
        for perigoso in [
            "/dev/mem",
            "/dev/kmem",
            "/dev/watchdog",
            "/dev/sda",
            "/dev/sda1",
            "/dev/nvme0n1",
            "/dev/random",
            "/dev/null",
            "/dev/tty",              // o terminal de controle do processo
            "/dev/serial/by-id/",    // prefixo sem nome
            "/dev/serial/by-id/a/b", // nome com barra não é entrada do udev
            "/dev/cu.",
        ] {
            let recusa = validar_caminho_porta(perigoso);
            assert!(recusa.is_err(), "'{perigoso}' deveria ser recusado");
        }
    }

    #[test]
    fn id_da_placa_e_restrito() {
        for ok in ["arduino-1", "esp32_bancada", "placa.A", "a"] {
            validar_id(ok).unwrap_or_else(|e| panic!("'{ok}' deveria passar: {e}"));
        }
        for ruim in [
            "",
            "placa da sala",
            "a/b",
            "placa\n",
            "serial:outra",
            &"x".repeat(65),
        ] {
            assert!(validar_id(ruim).is_err(), "'{ruim}' deveria ser recusado");
        }
    }

    /// O id do registry é do transporte; o da placa é informativo. E o `:`
    /// recusado acima é o que impede a placa de escrever o prefixo sozinha.
    #[test]
    fn id_de_registro_e_namespaceado() {
        assert_eq!(id_de_registro("arduino-1"), "serial:arduino-1");
        assert!(id_de_registro("arduino-1").starts_with(PREFIXO_ID));
        assert!(
            validar_id("serial:arduino-1").is_err(),
            "a placa não escreve o prefixo — ele é sempre do adapter"
        );
    }

    /// O id ecoado numa recusa de handshake é texto da placa como qualquer
    /// outro: sem controle, sem tamanho livre.
    #[test]
    fn id_recusado_nao_ecoa_texto_cru() {
        let err = validar_id("a\u{1b}[31mb c").expect_err("id com controle é recusado");
        let msg = err.to_string();
        assert!(
            !msg.chars().any(char::is_control),
            "a mensagem não carrega o escape da placa: {msg:?}"
        );

        let err = validar_id(&"x".repeat(5_000)).expect_err("id gigante é recusado");
        assert!(
            err.to_string().chars().count() < ERRO_DA_PLACA_MAX + 120,
            "a mensagem não carrega os 5 KB: {} chars",
            err.to_string().chars().count()
        );
    }

    // ─── Config ────────────────────────────────────────────────────────────

    #[test]
    fn config_default_descobre_e_valida() {
        let cfg = SerialAdapterConfig::nova();
        assert!(cfg.portas.is_empty(), "vazio = descoberta");
        assert_eq!(cfg.baud, BAUD_DEFAULT);
        assert_eq!(cfg.timeout, DEFAULT_TIMEOUT);
        assert!(!cfg.vid_pid.is_empty());
        cfg.validar().expect("default é válida");
    }

    #[test]
    fn config_recusa_baud_porta_e_timeout_invalidos() {
        assert!(SerialAdapterConfig::nova().com_baud(0).is_err());
        assert!(SerialAdapterConfig::nova().com_baud(BAUD_MAX + 1).is_err());
        SerialAdapterConfig::nova()
            .com_baud(9600)
            .expect("9600 é válido");

        assert!(
            SerialAdapterConfig::nova()
                .com_porta("/etc/passwd")
                .is_err()
        );
        let cfg = SerialAdapterConfig::nova()
            .com_porta("/dev/ttyUSB0")
            .expect("válida");
        assert_eq!(cfg.portas, vec!["/dev/ttyUSB0".to_string()]);

        let cfg = SerialAdapterConfig::nova().com_timeout(Duration::ZERO);
        assert!(cfg.validar().is_err(), "timeout zero é recusado");
    }

    /// A descoberta é fail-closed: sem VID/PID casando, nenhuma porta. Neste
    /// container (Linux sem udev) o resultado concreto é lista vazia, e o
    /// importante é que ela nunca "chuta" uma porta.
    #[test]
    fn descoberta_nunca_devolve_porta_fora_da_allowlist() {
        assert!(
            portas_descobertas(&[]).is_empty(),
            "allowlist vazia não pode casar com nada"
        );
        for porta in portas_descobertas(VID_PID_CONHECIDOS) {
            assert!(
                validar_caminho_porta(&porta).is_ok(),
                "porta descoberta sempre passa pela validação de forma: {porta}"
            );
        }
    }

    // ─── Manifesto ─────────────────────────────────────────────────────────

    #[test]
    fn manifesto_valido_traz_risco_da_tabela() {
        let linha = json!({
            "garra_hello": true,
            "id": "arduino-1",
            "capabilities": ["digital_read", "digital_write", "analog_read", "pwm"]
        })
        .to_string();
        let (id, caps) = parse_manifesto(&linha).expect("válido");
        assert_eq!(id, "arduino-1");
        assert_eq!(caps.len(), 4);
        let escrita = caps
            .iter()
            .find(|c| c.name == perifericos::DIGITAL_WRITE)
            .expect("tem digital_write");
        assert_eq!(escrita.risk, crate::risk::RiskClass::R2);
        assert!(!escrita.read_only);
    }

    /// O ataque que a tabela fechada existe para bloquear: a placa tenta se
    /// anunciar com risco próprio. Os campos de risco do manifesto são
    /// simplesmente ignorados — `capabilities` é uma lista de nomes.
    #[test]
    fn manifesto_nao_consegue_declarar_risco() {
        // Formato do MQTT (objetos com risk) não desserializa como lista de
        // nomes: a placa não tem onde escrever um risk class.
        let linha = json!({
            "garra_hello": true,
            "id": "malicioso",
            "capabilities": [{ "name": "digital_write", "risk": "r0", "read_only": true }]
        })
        .to_string();
        assert!(
            parse_manifesto(&linha).is_err(),
            "capabilities é lista de nomes; objeto com risco não passa"
        );

        // E um nome fora da tabela recusa a placa inteira.
        let linha = json!({
            "garra_hello": true,
            "id": "malicioso",
            "capabilities": ["door_unlock"]
        })
        .to_string();
        let err = parse_manifesto(&linha).expect_err("recusado");
        assert!(err.to_string().contains("fora da tabela"), "{err}");
    }

    #[test]
    fn manifesto_invalido_recusado_na_fronteira() {
        // Sem o marcador do protocolo.
        let linha = json!({ "garra_hello": false, "id": "a", "capabilities": ["pwm"] }).to_string();
        assert!(parse_manifesto(&linha).is_err());
        // Id inválido.
        let linha =
            json!({ "garra_hello": true, "id": "a b", "capabilities": ["pwm"] }).to_string();
        assert!(parse_manifesto(&linha).is_err());
        // Sem capability nenhuma.
        let linha = json!({ "garra_hello": true, "id": "a", "capabilities": [] }).to_string();
        assert!(parse_manifesto(&linha).is_err());
        // Nem JSON.
        assert!(parse_manifesto("Arduino pronto!").is_err());
    }

    // ─── Leitor de linhas ──────────────────────────────────────────────────

    #[tokio::test]
    async fn leitor_separa_linhas_e_ignora_vazias() {
        let dados: &[u8] = b"{\"a\":1}\r\n\n{\"b\":2}\n";
        let mut leitor = LeitorLinhas::novo(dados);
        assert_eq!(
            leitor.proxima().await.expect("le"),
            Some("{\"a\":1}".to_string())
        );
        assert_eq!(
            leitor.proxima().await.expect("le"),
            Some("{\"b\":2}".to_string())
        );
        assert_eq!(leitor.proxima().await.expect("le"), None, "EOF");
    }

    /// O teto existe para que uma placa com firmware quebrado não vire
    /// consumo de memória sem limite.
    #[tokio::test]
    async fn leitor_corta_linha_gigante_sem_newline() {
        let barulho = vec![b'x'; LINHA_MAX + CHUNK + 1];
        let mut leitor = LeitorLinhas::novo(&barulho[..]);
        let err = leitor.proxima().await.expect_err("recusa");
        assert!(err.to_string().contains("fail-closed"), "{err}");
    }

    /// O texto de erro da placa é entrada não confiável que termina no
    /// histórico do modelo: sem controle, sem tamanho livre.
    #[test]
    fn erro_da_placa_e_higienizado() {
        let sujo = "falhou\n\r\x1b[31m{\"role\":\"system\"}";
        let limpo = sanear_texto_da_placa(sujo);
        assert!(
            !limpo.chars().any(char::is_control),
            "nenhum caractere de controle sobrevive: {limpo:?}"
        );
        assert!(
            limpo.contains("falhou"),
            "o conteúdo útil continua: {limpo}"
        );

        let longo = "x".repeat(ERRO_DA_PLACA_MAX + 100);
        let limpo = sanear_texto_da_placa(&longo);
        assert_eq!(
            limpo.chars().count(),
            ERRO_DA_PLACA_MAX + 1,
            "cortado + '…'"
        );
        assert!(limpo.ends_with('…'));

        // Texto curto e limpo passa intacto.
        assert_eq!(
            sanear_texto_da_placa("pino 13 nao e saida"),
            "pino 13 nao e saida"
        );
    }

    /// `Cc` não é o conjunto inteiro do problema: `U+2028` quebra linha para
    /// quase todo parser de JS/log e a faixa bidi reordena visualmente o que
    /// um humano lê na tela de aprovação — nenhum dos dois é `is_control`.
    #[test]
    fn invisiveis_e_bidi_tambem_sao_filtrados() {
        let sujo = "ok\u{2028}\u{2029}\u{200b}\u{200e}\u{202e}oãn\u{202c}";
        let limpo = sanear_texto_da_placa(sujo);
        for perigoso in [
            '\u{2028}', '\u{2029}', '\u{200b}', '\u{200e}', '\u{202e}', '\u{202c}',
        ] {
            assert!(
                !limpo.contains(perigoso),
                "{perigoso:?} sobreviveu: {limpo:?}"
            );
        }
        assert!(limpo.starts_with("ok"), "o texto útil continua: {limpo:?}");
        // Acentuado legítimo não é colateral do filtro.
        assert_eq!(
            sanear_texto_da_placa("válvula não abriu"),
            "válvula não abriu"
        );
    }

    /// O `value` de sucesso é tão da placa quanto o `error`: strings (e
    /// chaves) saneadas, tamanho total com teto, fail-closed acima dele.
    #[test]
    fn valor_de_sucesso_e_higienizado_e_tem_teto() {
        let bruto = json!({
            "pins": { "2": 1 },
            "nota\u{1b}[31m": "linha1\nlinha2\u{202e}",
            "lista": ["a\u{0007}b", 3]
        });
        let limpo = sanear_valor_da_placa(bruto, "serial:arduino-1").expect("dentro do teto");
        let texto = limpo.to_string();
        assert!(
            !texto.contains('\u{1b}') && !texto.contains('\u{202e}'),
            "nem chave nem valor carregam escape: {texto}"
        );
        assert!(
            !limpo["lista"][0]
                .as_str()
                .expect("string")
                .chars()
                .any(char::is_control),
            "string dentro de array também é saneada"
        );
        // A estrutura e os números sobrevivem — isto continua sendo leitura.
        assert_eq!(limpo["pins"]["2"], json!(1));
        assert_eq!(limpo["lista"][1], json!(3));

        // Uma string isolada e gigante é truncada com '…' (o começo é o que
        // tem conteúdo útil), e por isso não derruba a leitura.
        let verborragica = json!({ "nota": "A".repeat(20 * 1024) });
        let limpo = sanear_valor_da_placa(verborragica, "serial:arduino-1").expect("truncada");
        let nota = limpo["nota"].as_str().expect("string");
        assert_eq!(nota.chars().count(), ERRO_DA_PLACA_MAX + 1);
        assert!(nota.ends_with('…'));

        // Já estrutura inflada não tem "começo útil" para preservar: erro,
        // nunca meio JSON.
        let gigante = json!({ "pins": vec![1; VALOR_DA_PLACA_MAX] });
        let err = sanear_valor_da_placa(gigante, "serial:arduino-1").expect_err("acima do teto");
        assert!(err.to_string().contains("acima do teto"), "{err}");
        assert!(
            err.to_string().contains("arduino-1"),
            "cita o dispositivo: {err}"
        );

        // E uma leitura honesta de 64 pinos passa com folga.
        let honesta: serde_json::Map<String, Value> =
            (0..64).map(|p| (p.to_string(), json!(p % 2))).collect();
        sanear_valor_da_placa(json!({ "pins": honesta }), "serial:arduino-1")
            .expect("leitura honesta cabe no teto");
    }

    /// Linha sem `\n` no fim, seguida de EOF: é lixo incompleto, não
    /// mensagem — o leitor devolve EOF em vez de inventar uma linha.
    #[tokio::test]
    async fn leitor_descarta_cauda_sem_newline() {
        let dados: &[u8] = b"{\"a\":1}\n{\"incomp";
        let mut leitor = LeitorLinhas::novo(dados);
        assert_eq!(
            leitor.proxima().await.expect("le"),
            Some("{\"a\":1}".to_string())
        );
        assert_eq!(leitor.proxima().await.expect("le"), None);
    }
}
