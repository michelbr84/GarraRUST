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
//!   declaradas à mão passam por [`validar_caminho_porta`]. Nunca varremos
//!   `/dev/tty*` inteiro.
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

/// Teto do texto de erro que a placa manda de volta.
const ERRO_DA_PLACA_MAX: usize = 300;

/// Higieniza o texto de erro vindo da placa antes de ele virar mensagem de
/// erro do adapter.
///
/// Esse texto é **entrada não confiável que chega ao modelo**: quem escreveu
/// o firmware (ou quem plugou a placa) escolhe cada byte, e o resultado vai
/// para o histórico da conversa como resposta de tool. Duas defesas simples:
/// caracteres de controle viram espaço (nada de quebra de linha forjando o
/// fim de uma mensagem, nada de sequência ANSI no terminal de quem estiver
/// lendo o log) e o comprimento é cortado, para uma placa verborrágica não
/// gastar o contexto do turno.
fn sanear_texto_da_placa(bruto: &str) -> String {
    let mut saneado: String = bruto
        .chars()
        .take(ERRO_DA_PLACA_MAX)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if bruto.chars().count() > ERRO_DA_PLACA_MAX {
        saneado.push('…');
    }
    saneado
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

/// Valida um caminho de porta antes de abri-lo.
///
/// Abrir um caminho arbitrário vindo de config é a superfície perigosa deste
/// adapter: um valor como `/etc/shadow` ou `../../dev/mem` não deve nem
/// chegar ao `open`. A regra é uma allowlist de forma — `/dev/...` no Unix,
/// `COM<n>` (ou `\\.\COM<n>`) no Windows — sem `..` em nenhum segmento e sem
/// bytes de controle.
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
    let e_unix = porta.strip_prefix("/dev/").is_some_and(|r| !r.is_empty());
    if !e_windows && !e_unix {
        return recusa("esperado '/dev/<porta>' (Unix) ou 'COM<n>' / '\\\\.\\COM<n>' (Windows)");
    }
    Ok(())
}

/// Valida o id que a placa declarou — ele vira chave do registry e aparece
/// em log e no inventário do agente, então nada de espaço, barra ou
/// caractere de controle.
fn validar_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 64 {
        return Err(HardwareError::Serial(format!(
            "id '{id}' inválido: esperado 1..=64 caracteres"
        )));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(HardwareError::Serial(format!(
            "id '{id}' inválido: só ASCII alfanumérico, '-', '_' e '.'"
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
fn parse_manifesto(linha: &str) -> Result<(String, Vec<Capability>)> {
    let manifesto: ManifestoSerial = serde_json::from_str(linha)
        .map_err(|e| HardwareError::Serial(format!("manifesto não é JSON válido: {e}")))?;
    if !manifesto.garra_hello {
        return Err(HardwareError::Serial(
            "manifesto sem 'garra_hello': true".to_string(),
        ));
    }
    validar_id(&manifesto.id)?;
    let caps = perifericos::resolver(&manifesto.capabilities, &manifesto.id)?;
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
    id: String,
    caps: Vec<Capability>,
    escrita: Arc<Mutex<Escrita>>,
    pendentes: Arc<Pendentes>,
    timeout: Duration,
    /// A porta (ou rótulo do stream, nos testes) — só para mensagem de erro.
    rotulo: String,
}

impl SerialDevice {
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
            Ok(Ok(RespostaPlaca::Valor(valor))) => Ok(valor),
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
        self.capability(capability)?;
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
/// Devolve o id registrado. Erro aqui significa "esta porta não é uma placa
/// Garra" e o chamador simplesmente segue para a próxima; o stream é
/// devolvido ao nada (fechado no drop).
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

    let (id, caps) =
        parse_manifesto(&linha).map_err(|e| HardwareError::Serial(format!("'{rotulo}': {e}")))?;

    // 2. Registro.
    let pendentes: Arc<Pendentes> = Arc::default();
    let device = SerialDevice {
        id: id.clone(),
        caps,
        escrita,
        pendentes: pendentes.clone(),
        timeout,
        rotulo: rotulo.clone(),
    };
    registry.register(Arc::new(device));
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
    // Linha espontânea com estado: alimenta o motor de automações (#1128).
    if let Some(estado) = bruta.state
        && let Some(bus) = bus
    {
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
            "/dev/serial/by-id/usb-Arduino-if00",
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

    #[test]
    fn id_da_placa_e_restrito() {
        for ok in ["arduino-1", "esp32_bancada", "placa.A", "a"] {
            validar_id(ok).unwrap_or_else(|e| panic!("'{ok}' deveria passar: {e}"));
        }
        for ruim in ["", "placa da sala", "a/b", "placa\n", &"x".repeat(65)] {
            assert!(validar_id(ruim).is_err(), "'{ruim}' deveria ser recusado");
        }
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
