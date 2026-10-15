//! Modo-maquina do `garra whatsapp link` (#1636): NDJSON no stdout.
//!
//! # Para que serve
//!
//! O `link` interativo desenha o QR num terminal e le o stdin. O app desktop
//! (epic #1181) nao tem terminal: ele lanca a CLI como sidecar e precisa de
//! um fluxo **nao interativo** que entregue o QR e as fases do pareamento a um
//! consumidor que nao e humano. Este modulo e a costura: um [`PairUi`]
//! ([`JsonUi`]) que, em vez de desenhar, emite **uma linha de JSON por
//! evento** no stdout.
//!
//! # O contrato NDJSON
//!
//! Cada linha e um objeto JSON com `schema` (hoje [`SCHEMA`]) e `event`:
//!
//! | `event`  | Campos | Significado |
//! |----------|--------|-------------|
//! | `status` | `status` (fase) | progresso: ver [`fase`] |
//! | `qr`     | `attempt`, `max_attempts`, `previous_expired`, `width`, `modules` | um QR novo; `modules` e a matriz como linhas de `0`/`1` (1 = modulo escuro), o payload cru NUNCA entra |
//! | `error`  | `code` (snake_case), `message` (pt-BR/EN) [+ extras] | falha terminal |
//! | `log`    | `message` | texto de progresso curto, cosmetico |
//!
//! O desenho do QR usa [`qr::modules`], o MESMO motor do `render` do terminal
//! — uma unica polaridade, sem segunda lib. O consumidor (fatia 2,
//! `garraia-desktop-core::onboarding`) pinta as celulas escuras num canvas e
//! **tolera** `event`/`status` que nao conhece, para que acrescentar fase no
//! futuro nao quebre uma versao antiga do app.
//!
//! # Por que stdout e so NDJSON
//!
//! O consumidor parseia o stdout linha a linha. Qualquer `println!` humano no
//! meio quebraria o parse, entao o caminho de modo-maquina NAO chama os
//! helpers de impressao do `link` interativo (header, instrucoes, QR
//! desenhado): ele passa por este modulo e nada mais escreve no stdout. Texto
//! livre do runner vira `log`; erro vira `error`. (stderr continua livre: o
//! consumidor nao o le como protocolo.)

use std::io::Write;

use garraia_channels::whatsapp_linked::qr;
use garraia_channels::whatsapp_linked::runner::{PairUi, RunError};
use serde_json::{Value, json};

use super::{Lang, t};

/// Versao do esquema NDJSON.
///
/// Sobe so quando um campo muda de forma incompativel. Acrescentar `event` ou
/// `status` novo nao exige bump: o consumidor ja tolera o desconhecido.
pub(super) const SCHEMA: u32 = 1;

/// Versao minima do Node.js que o modo vinculado exige (`whatsapp.rs`).
const NODE_MIN_VERSION: &str = "20";

/// As fases emitidas no campo `status` do evento `status`. Constantes para a
/// CLI e os testes nomearem sem string solta.
pub(super) mod fase {
    /// Procurando o Node na PATH.
    pub const DETECTING_NODE: &str = "detecting_node";
    /// Rodando `npm ci` (pode levar minutos).
    pub const INSTALLING_DEPS: &str = "installing_deps";
    /// `npm ci` concluiu.
    pub const DEPS_READY: &str = "deps_ready";
    /// Subindo o bridge Node.
    pub const STARTING_BRIDGE: &str = "starting_bridge";
    /// Aguardando a leitura de um QR.
    pub const WAITING_QR: &str = "waiting_qr";
    /// Conectando ao WhatsApp (antes do QR, ou reconectando).
    pub const CONNECTING: &str = "connecting";
    /// Pareamento aceito, sincronizando.
    pub const AUTHENTICATED: &str = "authenticated";
    /// Blob da sessao gravado em disco.
    pub const SESSION_SAVED: &str = "session_saved";
    /// `channels.whatsapp_linked.enabled = true` gravado.
    pub const CHANNEL_ENABLED: &str = "channel_enabled";
    /// Fim: vinculado e canal ligado.
    pub const DONE: &str = "done";
}

/// Monta o evento `status`.
pub(super) fn status_event(phase: &str) -> Value {
    json!({ "schema": SCHEMA, "event": "status", "status": phase })
}

/// Monta o evento `qr`: a matriz vira linhas de `0`/`1` (1 = modulo escuro).
///
/// Compacto e facil de pintar celula a celula; o payload cru NAO entra — ele
/// e credencial e o consumidor nao precisa dele para desenhar.
pub(super) fn qr_event(
    grid: &[Vec<bool>],
    attempt: u32,
    max_attempts: u32,
    previous_expired: bool,
) -> Value {
    let rows: Vec<String> = grid
        .iter()
        .map(|row| {
            row.iter()
                .map(|&dark| if dark { '1' } else { '0' })
                .collect()
        })
        .collect();
    json!({
        "schema": SCHEMA,
        "event": "qr",
        "attempt": attempt,
        "max_attempts": max_attempts,
        "previous_expired": previous_expired,
        "width": grid.len(),
        "modules": rows,
    })
}

/// Monta o evento `error`. `extra` acrescenta campos (ex.: `min_version`).
pub(super) fn error_event(code: &str, message: &str, extra: &[(&str, Value)]) -> Value {
    let mut obj = json!({ "schema": SCHEMA, "event": "error", "code": code, "message": message });
    if let Value::Object(map) = &mut obj {
        for (key, value) in extra {
            map.insert((*key).to_string(), value.clone());
        }
    }
    obj
}

/// Monta o evento `log`.
pub(super) fn log_event(message: &str) -> Value {
    json!({ "schema": SCHEMA, "event": "log", "message": message })
}

/// `code` snake_case + `message` legivel por [`RunError`], em pt-BR/EN.
///
/// Pura, para o teste fixar o codigo de cada variante sem subir bridge. O
/// `code` e o contrato de maquina; a `message` e para um humano eventualmente
/// ler. Nenhuma das duas carrega segredo: o `Display` do `BridgeError`/
/// `SessionError` ja e o mesmo que o `link` interativo imprime, redigido no
/// call site do runner.
pub(super) fn run_error_ndjson(err: &RunError, lang: Lang) -> (&'static str, String) {
    match err {
        RunError::QrExpired => (
            "qr_expired",
            t(lang, "Nenhum QR foi lido.", "No QR was scanned.").to_string(),
        ),
        RunError::Cancelled => ("cancelled", t(lang, "Cancelado.", "Cancelled.").to_string()),
        RunError::SessionDead { reason_code } => {
            let base = t(
                lang,
                "Esta sessao nao vale mais; leia um QR novo.",
                "This session is no longer valid; scan a new QR.",
            );
            let message = match reason_code {
                Some(code) => format!(
                    "{base} ({}: {code})",
                    t(lang, "codigo do WhatsApp", "WhatsApp code")
                ),
                None => base.to_string(),
            };
            ("session_dead", message)
        }
        RunError::MissingDependencies { dir } => (
            "missing_dependencies",
            format!(
                "{} {dir}",
                t(
                    lang,
                    "o bridge esta sem dependencias; rode npm ci em",
                    "the bridge is missing dependencies; run npm ci in",
                )
            ),
        ),
        RunError::Bridge(e) => ("bridge_failed", e.to_string()),
        RunError::Session(e) => ("session_error", e.to_string()),
    }
}

/// O evento `error` do "falta Node", com a versao minima anexada.
pub(super) fn missing_node_event(lang: Lang) -> Value {
    let message = t(
        lang,
        "Vincular o WhatsApp pessoal precisa do Node.js 20 ou mais novo \
(so este caminho precisa; o resto do GarraIA nao).",
        "Linking a personal WhatsApp needs Node.js 20 or newer (only this \
path does; the rest of GarraIA does not).",
    );
    error_event(
        "missing_node",
        message,
        &[("min_version", json!(NODE_MIN_VERSION))],
    )
}

/// [`PairUi`] que emite NDJSON em vez de desenhar.
///
/// Mesma disciplina do `TerminalUi`/`spinner.rs`: escreve num sink `Write`
/// (o teste captura sem mexer no stdout do processo), nunca emite ANSI, nunca
/// esconde o cursor.
pub(super) struct JsonUi {
    lang: Lang,
    sink: Box<dyn Write + Send>,
    /// Evita repetir `status: connecting` a cada tick. Re-arma apos qr/log,
    /// para que uma reconexao depois de uma queda volte a anunciar a fase.
    connecting_emitted: bool,
    /// Ultima tentativa de QR ja anunciada como `waiting_qr`, para o contador
    /// por segundo nao virar uma linha por segundo.
    last_waiting_attempt: Option<u32>,
}

impl JsonUi {
    /// UI de producao: escreve no stdout do processo.
    pub(super) fn new(lang: Lang) -> Self {
        Self {
            lang,
            sink: Box::new(std::io::stdout()),
            connecting_emitted: false,
            last_waiting_attempt: None,
        }
    }

    /// UI com sink injetado, para o teste ler as linhas emitidas.
    #[cfg(test)]
    pub(super) fn with_sink(lang: Lang, sink: Box<dyn Write + Send>) -> Self {
        Self {
            lang,
            sink,
            connecting_emitted: false,
            last_waiting_attempt: None,
        }
    }

    /// Escreve um evento como UMA linha terminada em `\n`.
    ///
    /// `to_string` de um `Value` nunca emite `\n` literal (newline vira `\n`
    /// escapado dentro da string), entao cada evento ocupa exatamente uma
    /// linha fisica — o que o consumidor assume ao ler linha a linha.
    fn emit(&mut self, value: &Value) {
        let _ = writeln!(self.sink, "{value}");
        let _ = self.sink.flush();
    }

    /// Emite um evento `status` com a fase dada.
    pub(super) fn emit_status(&mut self, phase: &str) {
        self.emit(&status_event(phase));
    }

    /// Emite um evento `error` (com campos extras opcionais).
    pub(super) fn emit_error(&mut self, code: &str, message: &str, extra: &[(&str, Value)]) {
        self.emit(&error_event(code, message, extra));
    }

    /// Emite o `error` de "falta Node", com `min_version`.
    pub(super) fn emit_missing_node(&mut self) {
        self.emit(&missing_node_event(self.lang));
    }

    /// Traduz um [`RunError`] em evento `error` e o emite.
    pub(super) fn emit_run_error(&mut self, err: &RunError) {
        let (code, message) = run_error_ndjson(err, self.lang);
        self.emit_error(code, &message, &[]);
    }
}

impl PairUi for JsonUi {
    fn status(&mut self, line: &str) {
        // Texto livre de progresso do runner -> `log`. Re-arma o `connecting`:
        // depois de um retry anunciado por aqui, a proxima fase conectando
        // volta a aparecer como `status`.
        self.connecting_emitted = false;
        self.emit(&log_event(line));
    }

    fn qr(&mut self, data: &str, attempt: u32, max_attempts: u32, previous_expired: bool) {
        self.connecting_emitted = false;
        match qr::modules(data) {
            Ok(grid) => self.emit(&qr_event(&grid, attempt, max_attempts, previous_expired)),
            // Inalcancavel com payload real do bridge; nao pode matar o
            // pareamento. O proximo QR pode codificar.
            Err(e) => self.emit(&log_event(&format!("falha ao gerar o QR: {e}"))),
        }
    }

    fn waiting(&mut self, attempt: u32, _max_attempts: u32, _seconds_left: u64) {
        // Um `waiting_qr` por QR (dedup na tentativa): o marcador de fase "QR
        // na tela, aguardando leitura". O contador por segundo NAO entra —
        // seriam ~20 linhas de ruido por QR; o consumidor roda o proprio
        // relogio se quiser contagem regressiva. O `qr` ja trouxe a matriz.
        if self.last_waiting_attempt != Some(attempt) {
            self.last_waiting_attempt = Some(attempt);
            self.emit_status(fase::WAITING_QR);
        }
    }

    fn connecting(&mut self, _elapsed_secs: u64, _giving_up_in_secs: u64) {
        // Uma vez por fase conectando; o consumidor mapeia para "Conectando".
        if !self.connecting_emitted {
            self.connecting_emitted = true;
            self.emit_status(fase::CONNECTING);
        }
    }

    fn authenticated(&mut self) {
        self.connecting_emitted = false;
        self.emit_status(fase::AUTHENTICATED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_channels::whatsapp_linked::bridge::BridgeError;
    use std::sync::{Arc, Mutex};

    /// Sink de teste: acumula os bytes num buffer compartilhado para o teste
    /// ler as linhas depois de os eventos sairem.
    #[derive(Clone)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Buffer {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Vec::new())))
        }
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().expect("lock").clone()).expect("utf8")
        }
    }

    impl Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Um QR real da fatia 1, para os testes cruzarem a matriz com o motor.
    const QR_DATA: &str = "2@GarraIA/TEST/FIXTURE,abcdef0123456789,==";

    #[test]
    fn every_emitted_event_is_one_valid_json_line_with_schema_1() {
        let buf = Buffer::new();
        let mut ui = JsonUi::with_sink(Lang::Pt, Box::new(buf.clone()));
        ui.emit_status(fase::DETECTING_NODE);
        PairUi::connecting(&mut ui, 0, 90);
        PairUi::qr(&mut ui, QR_DATA, 1, 5, false);
        PairUi::authenticated(&mut ui);
        ui.emit_status(fase::SESSION_SAVED);
        ui.emit_run_error(&RunError::QrExpired);

        let text = buf.text();
        assert!(text.ends_with('\n'), "a saida termina em newline");
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.len() >= 6, "um evento por linha: {lines:?}");
        for line in &lines {
            let value: Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("linha nao e JSON valido: {line:?} ({e})"));
            assert_eq!(
                value["schema"],
                json!(1),
                "toda linha carrega schema 1: {line}"
            );
            assert!(
                value.get("event").is_some(),
                "toda linha tem `event`: {line}"
            );
        }
    }

    #[test]
    fn the_qr_event_carries_the_matrix_as_zero_one_rows_and_never_the_payload() {
        let grid = qr::modules(QR_DATA).expect("modules");
        let value = qr_event(&grid, 2, 5, true);
        assert_eq!(value["event"], json!("qr"));
        assert_eq!(value["attempt"], json!(2));
        assert_eq!(value["max_attempts"], json!(5));
        assert_eq!(value["previous_expired"], json!(true));
        assert_eq!(value["width"], json!(grid.len()));

        let rows = value["modules"].as_array().expect("modules e array");
        assert_eq!(rows.len(), grid.len(), "uma linha por linha da matriz");
        for (row_str, grid_row) in rows.iter().zip(&grid) {
            let s = row_str.as_str().expect("linha e string");
            assert_eq!(s.len(), grid_row.len(), "largura da linha");
            assert!(s.chars().all(|c| c == '0' || c == '1'), "so 0/1: {s:?}");
            // 1 = modulo escuro, exatamente a polaridade de `qr::modules`.
            for (ch, &dark) in s.chars().zip(grid_row) {
                assert_eq!(ch == '1', dark);
            }
        }
        // O payload cru e credencial: nao pode aparecer em lugar nenhum.
        let serialized = serde_json::to_string(&value).expect("serialize");
        assert!(
            !serialized.contains(QR_DATA),
            "o payload cru nao pode vazar no evento qr"
        );
    }

    #[test]
    fn run_error_maps_each_variant_to_a_snake_case_code() {
        let cases: Vec<(RunError, &str)> = vec![
            (RunError::QrExpired, "qr_expired"),
            (RunError::Cancelled, "cancelled"),
            (
                RunError::SessionDead {
                    reason_code: Some(401),
                },
                "session_dead",
            ),
            (
                RunError::MissingDependencies { dir: "/x".into() },
                "missing_dependencies",
            ),
            (
                RunError::Bridge(BridgeError::ToolMissing("node")),
                "bridge_failed",
            ),
        ];
        for (err, expected) in cases {
            let (code, message) = run_error_ndjson(&err, Lang::Pt);
            assert_eq!(code, expected, "{err:?}");
            assert!(!message.is_empty(), "mensagem vazia para {err:?}");
        }
    }

    #[test]
    fn the_session_dead_message_includes_the_whatsapp_code() {
        let (_, message) = run_error_ndjson(
            &RunError::SessionDead {
                reason_code: Some(401),
            },
            Lang::Pt,
        );
        assert!(
            message.contains("401"),
            "o codigo cru precisa aparecer: {message}"
        );
    }

    #[test]
    fn missing_node_event_carries_the_minimum_version() {
        let value = missing_node_event(Lang::Pt);
        assert_eq!(value["event"], json!("error"));
        assert_eq!(value["code"], json!("missing_node"));
        assert_eq!(value["min_version"], json!("20"));
    }

    #[test]
    fn error_messages_follow_the_language() {
        let (_, pt) = run_error_ndjson(&RunError::QrExpired, Lang::Pt);
        let (_, en) = run_error_ndjson(&RunError::QrExpired, Lang::En);
        assert_ne!(pt, en, "a mensagem muda com o idioma");
    }

    #[test]
    fn connecting_is_announced_once_then_rearmed_by_a_log() {
        let buf = Buffer::new();
        let mut ui = JsonUi::with_sink(Lang::Pt, Box::new(buf.clone()));
        PairUi::connecting(&mut ui, 0, 90);
        PairUi::connecting(&mut ui, 5, 85); // mesmo trecho: nao repete
        PairUi::status(&mut ui, "a sessao caiu — tentando de novo"); // re-arma
        PairUi::connecting(&mut ui, 10, 80); // nova fase: aparece de novo

        let connecting = buf
            .text()
            .lines()
            .filter(|l| l.contains(r#""status":"connecting""#))
            .count();
        assert_eq!(connecting, 2, "uma vez por trecho conectando");
    }

    #[test]
    fn waiting_emits_one_waiting_qr_per_attempt_not_per_second() {
        let buf = Buffer::new();
        let mut ui = JsonUi::with_sink(Lang::Pt, Box::new(buf.clone()));
        // Tres ticks da mesma tentativa: uma linha so.
        PairUi::waiting(&mut ui, 1, 5, 19);
        PairUi::waiting(&mut ui, 1, 5, 18);
        PairUi::waiting(&mut ui, 1, 5, 17);
        // Tentativa nova: outra linha.
        PairUi::waiting(&mut ui, 2, 5, 19);

        let waiting = buf
            .text()
            .lines()
            .filter(|l| l.contains(r#""status":"waiting_qr""#))
            .count();
        assert_eq!(waiting, 2, "um waiting_qr por tentativa, nao por segundo");
    }
}
