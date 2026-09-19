//! Tipos serde do protocolo NDJSON v1 falado com o bridge Node/Baileys.
//!
//! O contrato completo esta em `docs/whatsapp.md` §Protocolo e no ADR 0023.
//! Aqui vale repetir so o que muda a forma destes tipos:
//!
//! - **Uma mensagem JSON por linha**, campo obrigatorio `type`. Por isso as
//!   duas enums sao *internally tagged* (`#[serde(tag = "type")]`).
//! - **Ordem estrita**: `started` e sempre a primeira linha; o Rust so manda
//!   `session_load` depois dela, e `start` so depois do `session_load`.
//! - O stdout do bridge e **somente** NDJSON. Texto livre ali e erro de
//!   protocolo, nao aviso.
//!
//! # Forward-compat
//!
//! Evento com `type` desconhecido vira [`BridgeEvent::Unknown`] em vez de erro
//! de parse. Um bridge mais novo que aprenda um evento novo nao derruba uma CLI
//! mais velha — e a incompatibilidade que realmente importa (`protocol != 1`)
//! e verificada explicitamente no `started`, com mensagem clara.
//!
//! # PII
//!
//! Nenhum tipo aqui deriva `Debug` de graca quando carrega identificador ou
//! conteudo de mensagem. [`Jid`] imprime so os 4 ultimos digitos,
//! [`InboundMessage`] imprime forma sem conteudo, e [`SessionBlob`] imprime
//! `<redacted>` — ver [`crate::whatsapp_linked::session`].

use serde::{Deserialize, Serialize};
use std::fmt;

use super::session::SessionBlob;

/// Versao de protocolo que esta CLI fala. Diferenca no `started` e recusa.
pub const PROTOCOL_VERSION: u32 = 1;

/// Teto de uma linha NDJSON. Igual ao `MAX_CONNECTOR_FRAME_BYTES` de
/// `crate::protocol`: a sessao do Baileys serializada e a maior coisa que
/// trafega aqui e cabe folgada em 256 KiB; alem disso e ou bug ou alguem
/// tentando fazer o leitor alocar sem limite.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;

/// Expiracao default de um QR quando o bridge nao informa (segundos).
///
/// 20 s e o intervalo do proprio Baileys; o campo existe para o bridge poder
/// contradizer isso sem a CLI precisar de release nova.
pub const DEFAULT_QR_EXPIRY_SECS: u64 = 20;

/// JID do WhatsApp. `Debug` mostra so os 4 ultimos digitos.
///
/// O JID proprio *e* o numero de telefone do usuario. Ele passa por `connected`
/// e por toda `message`, entao um `#[derive(Debug)]` aqui seria um vazamento a
/// um `tracing::debug!` de distancia.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Jid(String);

impl Jid {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Valor cru. Existe para o canal do gateway poder endereçar a conversa;
    /// nunca deve ir para log.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Os 4 ultimos digitos, a unica forma que pode ser logada.
    ///
    /// Mesma politica do `redactWhatsAppId` do Hermes e do campo
    /// `phone_last4` do proprio protocolo.
    pub fn last4(&self) -> String {
        let digits: Vec<char> = self.0.chars().filter(|c| c.is_ascii_digit()).collect();
        let tail: String = digits.iter().rev().take(4).rev().collect();
        if tail.is_empty() {
            "????".to_string()
        } else {
            tail
        }
    }
}

impl fmt::Debug for Jid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Jid(***{})", self.last4())
    }
}

/// Estados que o bridge reporta em `status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeState {
    Connecting,
    WaitingScan,
    Authenticated,
    Syncing,
    Connected,
    Reconnecting,
    Disconnected,
    /// `status` com um estado que esta CLI nao conhece.
    #[serde(other)]
    Unknown,
}

/// Motivo de queda, ja normalizado pelo bridge (o codigo numerico do Baileys
/// vem separado em `reason_code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisconnectReason {
    RestartRequired,
    Network,
    Timeout,
    LoggedOut,
    Replaced,
    #[serde(other)]
    Unknown,
}

/// Codigo de erro reportado pelo bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    NotConnected,
    SendFailed,
    Protocol,
    Internal,
    #[serde(other)]
    Unknown,
}

/// Nivel de um evento `log`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
    #[serde(other)]
    Unknown,
}

/// Mensagem recebida. So o slice do gateway consome isto.
///
/// `Debug` manual: `text`, `sender_phone` e os JIDs sao conteudo de terceiros
/// e numero de telefone. O que sobra e a **forma** — util para diagnostico,
/// inutil para quem quer os dados.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct InboundMessage {
    pub id: String,
    pub chat_jid: Jid,
    pub sender_jid: Jid,
    /// E.164 **com o `+`** (`+5511999998888`). `null` para remetente `@lid`,
    /// que nao expoe numero.
    #[serde(default)]
    pub sender_phone: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    pub timestamp: i64,
    #[serde(default)]
    pub is_group: bool,
    #[serde(default)]
    pub from_me: bool,
    #[serde(default)]
    pub push_name: Option<String>,
    /// Sempre presente no JSON, `null` para texto puro. Quando ha midia:
    /// `image`, `video`, `audio`, `document`, `sticker`, `location` ou
    /// `contact`. Legenda de midia **nao** e entregue: nesse caso `text` e
    /// `None` e so `media_kind` diz o que chegou.
    #[serde(default)]
    pub media_kind: Option<String>,
}

impl fmt::Debug for InboundMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InboundMessage")
            .field("chat", &format_args!("***{}", self.chat_jid.last4()))
            .field("sender", &format_args!("***{}", self.sender_jid.last4()))
            .field("is_group", &self.is_group)
            .field("from_me", &self.from_me)
            .field("has_text", &self.text.is_some())
            .field("media_kind", &self.media_kind)
            .finish_non_exhaustive()
    }
}

/// Bridge → Rust.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    Started {
        protocol: u32,
        #[serde(default)]
        bridge_version: Option<String>,
        /// `None` quando o bridge subiu **sem `node_modules`**. O bridge ainda
        /// responde o handshake nesse estado de proposito (e o que permite o
        /// `--protocol-check` funcionar sem deps), entao `None` aqui nao e
        /// falha: e o sinal de que falta `npm ci`, e a CLI diz exatamente isso
        /// em vez de estourar opaco no primeiro `start`.
        #[serde(default)]
        baileys_version: Option<String>,
        #[serde(default)]
        node_version: Option<String>,
    },
    Qr {
        data: String,
        #[serde(default = "default_qr_expiry")]
        expires_in_secs: u64,
        #[serde(default = "first_attempt")]
        attempt: u32,
    },
    Status {
        state: BridgeState,
        #[serde(default)]
        detail: Option<String>,
    },
    Authenticated,
    Connected {
        /// Sempre presente no JSON, mas **pode ser `null`**: o proprio JID nem
        /// sempre e conhecido no instante em que o socket abre.
        #[serde(default)]
        jid: Option<Jid>,
        #[serde(default)]
        phone_last4: Option<String>,
        #[serde(default)]
        pushname: Option<String>,
    },
    Disconnected {
        #[serde(default)]
        reason_code: Option<i64>,
        reason: DisconnectReason,
        #[serde(default)]
        will_retry: bool,
        #[serde(default)]
        retry_in_ms: Option<u64>,
    },
    LoggedOut,
    SessionUpdate {
        session: SessionBlob,
        /// Comeca em 1 e cresce **por processo**, nao entre reinicios. NAO
        /// serve como versao do blob em disco e nao e persistido: o snapshot e
        /// sempre completo, e o ultimo que chegou vence.
        #[serde(default)]
        seq: u64,
    },
    Message(Box<InboundMessage>),
    Sent {
        /// Eco do que o comando mandou — pode ser `null`.
        #[serde(default)]
        request_id: Option<String>,
        /// Id da mensagem no WhatsApp; `null` quando o servidor nao devolveu.
        #[serde(default)]
        id: Option<String>,
    },
    Error {
        #[serde(default)]
        request_id: Option<String>,
        code: ErrorCode,
        message: String,
    },
    Log {
        level: LogLevel,
        message: String,
    },
    /// Resposta do bridge a um [`BridgeCommand::Ping`]. Sem carga: o que ela
    /// prova e que o laco de stdout do filho esta de pe, nao que algo mudou.
    Pong,
    /// `type` que esta versao nao conhece. Ignorado pelo driver.
    #[serde(other)]
    Unknown,
}

fn default_qr_expiry() -> u64 {
    DEFAULT_QR_EXPIRY_SECS
}

fn first_attempt() -> u32 {
    1
}

/// Modo de operacao pedido no comando `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartMode {
    /// Pareamento: emite `qr` ate `connected`, faz o flush final e sai 0.
    Pair,
    /// Longa duracao: reconecta em toda queda exceto `logged_out`.
    Serve,
}

/// Rust → Bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeCommand {
    /// Primeiro comando, sempre. `None` = sem sessao (pareamento).
    SessionLoad {
        session: Option<SessionBlob>,
    },
    Start {
        mode: StartMode,
    },
    Send {
        request_id: String,
        chat_jid: Jid,
        text: String,
    },
    Read {
        chat_jid: Jid,
        ids: Vec<String>,
    },
    Typing {
        chat_jid: Jid,
        on: bool,
    },
    Logout,
    /// Prova de vida pedida pelo driver. E a resposta —
    /// [`BridgeEvent::Pong`] ou qualquer outro evento — que renova o relogio
    /// de silencio pos-`connected` do `serve` (issue #1275): a ponte real pode
    /// ficar quieta por horas numa conta sem mensagens, e a unica forma de
    /// distinguir silencio saudavel de filho emudecido e pedir que ele fale.
    /// Um bridge velho que nao conhece `ping` responde com um evento `error`
    /// — que tambem prova vida, e por isso serve.
    Ping,
    /// Fecha o socket **sem** deslogar: a sessao continua valida.
    Shutdown,
}

/// Codigos de saida do bridge, conforme o protocolo v1.
///
/// **O codigo de saida e quem distingue "eu pedi logout" de "o servidor me
/// deslogou".** Os dois emitem `logged_out`; so o codigo diz se a sessao ainda
/// vale. Por isso o driver nunca apaga sessao ao ver o evento — ele espera o
/// processo terminar.
pub mod exit_code {
    /// Encerramento normal: `pair` concluido, `shutdown`, `logout` **pedido
    /// por nos**, e tambem o codigo 440 (`connectionReplaced`, outro aparelho
    /// assumiu). Em nenhum desses casos a sessao deve ser apagada.
    pub const NORMAL: i32 = 0;
    /// Erro fatal de inicializacao (node, deps, protocolo).
    pub const FATAL_INIT: i32 = 1;
    /// **Sessao morta**: 401 (`loggedOut`), 403 e 419 — os `UNAUTHORIZED_CODES`
    /// do Baileys. A sessao e apagada e um QR novo passa a ser obrigatorio.
    pub const SESSION_DEAD: i32 = 2;
    /// Erro de protocolo (linha invalida ou grande demais).
    pub const PROTOCOL: i32 = 3;
}

/// Codigos do Baileys que significam **sessao morta** — os
/// `UNAUTHORIZED_CODES` da propria biblioteca.
///
/// Existem aqui, e nao so no bridge, porque o codigo de saida pode se perder:
/// um filho morto por sinal (OOM killer, `SIGABRT` do runtime) nao tem codigo
/// de saida nenhum. Nesse caso o `reason_code` do ultimo `disconnected` e a
/// unica evidencia que sobra, e ignora-la significaria tentar reconectar para
/// sempre com uma sessao que o servidor ja recusou.
pub const UNAUTHORIZED_REASON_CODES: &[i64] = &[401, 403, 419];

/// A sessao morreu?
///
/// `exit_code` e a prova primaria; `reason_code` e a secundaria, para quando a
/// primeira nao existe. Um `logged_out` **sem** nenhuma das duas e um logout
/// que nos pedimos, ou um 440 (`connectionReplaced`): nos dois a sessao
/// gravada continua valendo e nada pode ser apagado.
pub fn session_is_dead(exit_code: Option<i32>, reason_code: Option<i64>) -> bool {
    if exit_code == Some(exit_code::SESSION_DEAD) {
        return true;
    }
    exit_code.is_none() && reason_code.is_some_and(|c| UNAUTHORIZED_REASON_CODES.contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(line: &str) -> BridgeEvent {
        let ev: BridgeEvent = serde_json::from_str(line).expect("parse");
        let back = serde_json::to_string(&ev).expect("serialize");
        let again: BridgeEvent = serde_json::from_str(&back).expect("reparse");
        assert_eq!(ev, again, "round-trip precisa ser estavel: {line}");
        ev
    }

    #[test]
    fn started_round_trips() {
        let ev = round_trip(
            r#"{"type":"started","protocol":1,"bridge_version":"1.0.0","baileys_version":"7.0.0-rc13","node_version":"v22.1.0"}"#,
        );
        assert!(matches!(ev, BridgeEvent::Started { protocol: 1, .. }));
    }

    /// `baileys_version: null` e o sinal de "sem node_modules", nao um erro de
    /// parse. Se este teste quebrar, `garra whatsapp` volta a falhar opaco numa
    /// instalacao sem deps.
    #[test]
    fn started_without_node_modules_parses_with_a_null_baileys_version() {
        let ev: BridgeEvent = serde_json::from_str(
            r#"{"type":"started","protocol":1,"bridge_version":"1.0.0","baileys_version":null,"node_version":"v22.1.0"}"#,
        )
        .expect("parse");
        match ev {
            BridgeEvent::Started {
                baileys_version, ..
            } => assert!(baileys_version.is_none()),
            other => panic!("esperava started, veio {other:?}"),
        }
    }

    #[test]
    fn qr_defaults_expiry_and_attempt() {
        let ev = round_trip(r#"{"type":"qr","data":"2@abc"}"#);
        match ev {
            BridgeEvent::Qr {
                expires_in_secs,
                attempt,
                ref data,
            } => {
                assert_eq!(expires_in_secs, DEFAULT_QR_EXPIRY_SECS);
                assert_eq!(attempt, 1);
                assert_eq!(data, "2@abc");
            }
            other => panic!("esperava qr, veio {other:?}"),
        }
    }

    #[test]
    fn every_event_type_round_trips() {
        let lines = [
            r#"{"type":"qr","data":"2@x","expires_in_secs":20,"attempt":2}"#,
            r#"{"type":"status","state":"waiting_scan","detail":"aguardando"}"#,
            r#"{"type":"authenticated"}"#,
            r#"{"type":"connected","jid":"5511999998888@s.whatsapp.net","phone_last4":"8888","pushname":"Ana"}"#,
            r#"{"type":"connected","jid":null,"phone_last4":null,"pushname":null}"#,
            r#"{"type":"sent","request_id":null,"id":null}"#,
            r#"{"type":"started","protocol":1,"bridge_version":"1.0.0","baileys_version":null,"node_version":"v22.1.0"}"#,
            r#"{"type":"message","id":"B","chat_jid":"1@s.whatsapp.net","sender_jid":"2@lid","sender_phone":null,"text":null,"media_kind":"image","timestamp":1758000000,"is_group":true,"from_me":false,"push_name":null}"#,
            r#"{"type":"disconnected","reason_code":515,"reason":"restart_required","will_retry":true,"retry_in_ms":1000}"#,
            r#"{"type":"logged_out"}"#,
            r#"{"type":"session_update","session":"eyJhIjoxfQ==","seq":3}"#,
            r#"{"type":"message","id":"AAA","chat_jid":"1@s.whatsapp.net","sender_jid":"2@s.whatsapp.net","sender_phone":"+5511999998888","text":"oi","timestamp":1758000000,"is_group":false,"from_me":false,"push_name":"Ana"}"#,
            r#"{"type":"sent","request_id":"r1","id":"MID"}"#,
            r#"{"type":"error","code":"not_connected","message":"socket fechado"}"#,
            r#"{"type":"log","level":"warn","message":"reconectando"}"#,
            r#"{"type":"pong"}"#,
        ];
        for line in lines {
            round_trip(line);
        }
    }

    #[test]
    fn unknown_event_type_is_not_a_parse_error() {
        let ev: BridgeEvent =
            serde_json::from_str(r#"{"type":"presence","jid":"x"}"#).expect("parse");
        assert_eq!(ev, BridgeEvent::Unknown);
    }

    #[test]
    fn unknown_enum_members_degrade_instead_of_failing() {
        let ev: BridgeEvent =
            serde_json::from_str(r#"{"type":"status","state":"hibernating"}"#).expect("parse");
        assert!(matches!(
            ev,
            BridgeEvent::Status {
                state: BridgeState::Unknown,
                ..
            }
        ));
    }

    #[test]
    fn commands_serialize_to_the_documented_shape() {
        let cmd = BridgeCommand::SessionLoad { session: None };
        assert_eq!(
            serde_json::to_string(&cmd).expect("ser"),
            r#"{"type":"session_load","session":null}"#
        );

        let cmd = BridgeCommand::Start {
            mode: StartMode::Pair,
        };
        assert_eq!(
            serde_json::to_string(&cmd).expect("ser"),
            r#"{"type":"start","mode":"pair"}"#
        );

        let cmd = BridgeCommand::Shutdown;
        assert_eq!(
            serde_json::to_string(&cmd).expect("ser"),
            r#"{"type":"shutdown"}"#
        );
    }

    #[test]
    fn commands_round_trip() {
        let cmds = [
            BridgeCommand::SessionLoad {
                session: Some(SessionBlob::new("eyJhIjoxfQ==")),
            },
            BridgeCommand::Start {
                mode: StartMode::Serve,
            },
            BridgeCommand::Send {
                request_id: "r1".into(),
                chat_jid: Jid::new("1@s.whatsapp.net"),
                text: "oi".into(),
            },
            BridgeCommand::Read {
                chat_jid: Jid::new("1@s.whatsapp.net"),
                ids: vec!["A".into()],
            },
            BridgeCommand::Typing {
                chat_jid: Jid::new("1@s.whatsapp.net"),
                on: true,
            },
            BridgeCommand::Logout,
            BridgeCommand::Ping,
            BridgeCommand::Shutdown,
        ];
        for cmd in cmds {
            let line = serde_json::to_string(&cmd).expect("ser");
            let back: BridgeCommand = serde_json::from_str(&line).expect("de");
            assert_eq!(cmd, back);
        }
    }

    /// Tabela da decisao mais cara do modulo: apagar ou nao apagar a sessao.
    #[test]
    fn session_death_decision_table() {
        let cases: &[(Option<i32>, Option<i64>, bool, &str)] = &[
            (Some(2), Some(401), true, "exit 2 e a prova primaria"),
            (Some(2), None, true, "exit 2 basta sozinho"),
            (Some(0), None, false, "logout que nos pedimos"),
            (Some(0), Some(440), false, "440: outro aparelho assumiu"),
            (
                Some(0),
                Some(401),
                false,
                "exit 0 vence: o bridge saiu limpo",
            ),
            (Some(1), None, false, "erro fatal de init nao mata a sessao"),
            (Some(3), None, false, "erro de protocolo nao mata a sessao"),
            (
                None,
                Some(401),
                true,
                "sem codigo, o 401 e a evidencia que resta",
            ),
            (None, Some(403), true, "403 tambem e UNAUTHORIZED"),
            (None, Some(419), true, "419 tambem e UNAUTHORIZED"),
            (None, Some(515), false, "515 e restart_required, nao morte"),
            (None, Some(428), false, "428 e rede"),
            (
                None,
                None,
                false,
                "sem evidencia nenhuma, nao se apaga nada",
            ),
        ];
        for &(exit_code, reason_code, expected, why) in cases {
            assert_eq!(
                session_is_dead(exit_code, reason_code),
                expected,
                "{why} (exit={exit_code:?} reason={reason_code:?})"
            );
        }
    }

    #[test]
    fn jid_debug_shows_only_the_last_four_digits() {
        let jid = Jid::new("5511999998888@s.whatsapp.net");
        let shown = format!("{jid:?}");
        assert_eq!(shown, "Jid(***8888)");
        assert!(
            !shown.contains("5511999"),
            "o numero inteiro nao pode aparecer: {shown}"
        );
    }

    #[test]
    fn jid_without_digits_degrades_to_a_placeholder() {
        assert_eq!(Jid::new("status@broadcast").last4(), "????");
    }

    #[test]
    fn inbound_message_debug_hides_content_and_numbers() {
        let msg = InboundMessage {
            id: "AAA".into(),
            chat_jid: Jid::new("5511999998888@s.whatsapp.net"),
            sender_jid: Jid::new("5511777776666@s.whatsapp.net"),
            sender_phone: Some("+5511777776666".into()),
            text: Some("senha do banco e 1234".into()),
            timestamp: 0,
            is_group: false,
            from_me: false,
            push_name: Some("Ana".into()),
            media_kind: None,
        };
        let shown = format!("{msg:?}");
        for forbidden in ["senha do banco", "+5511777776666", "5511999998888", "Ana"] {
            assert!(
                !shown.contains(forbidden),
                "Debug vazou {forbidden:?}: {shown}"
            );
        }
        assert!(shown.contains("***8888"));
        assert!(shown.contains("has_text: true"));
    }
}
