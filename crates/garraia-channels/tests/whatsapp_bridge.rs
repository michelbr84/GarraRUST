//! Ciclo de vida do bridge do WhatsApp vinculado, contra a fixture Python.
//!
//! Molde: `crates/garraia-agents/tests/mcp_lifecycle.rs`. A fixture
//! (`tests/fixtures/fake_whatsapp_bridge.py`, stdlib pura) fala o protocolo
//! NDJSON v1 de verdade, entao estes testes exercitam o caminho inteiro —
//! `spawn` com `env_clear` + allowlist, enquadramento com teto, maquina de
//! estados, store cifrado — **sem Node e sem telefone**. Baileys real e
//! telefone real seguem sendo validacao manual, documentada em
//! `docs/whatsapp.md`.
//!
//! `#[cfg(unix)]`: o `pre_exec` (PDEATHSIG) e o `python3` na PATH sao premissas
//! de Unix. No Windows o CI nao roda estes testes.
#![cfg(unix)]
#![cfg(feature = "whatsapp-linked")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use garraia_channels::whatsapp_linked::bridge::{BridgeError, BridgeLauncher};
use garraia_channels::whatsapp_linked::runner::{
    InboundSink, PairOptions, PairUi, RunError, SilentUi, pair, pair_with, serve,
};
use garraia_channels::whatsapp_linked::{
    BridgeCommand, InboundMessage, Jid, SessionKey, SessionStore,
};
use tokio::process::Command;
use tokio::sync::watch;

/// Lanca a fixture no lugar do `node bridge.mjs`.
struct FixtureLauncher {
    scenario: &'static str,
    qr_expires: f64,
    hang_secs: f64,
    dir: PathBuf,
}

impl FixtureLauncher {
    fn new(scenario: &'static str, dir: PathBuf) -> Self {
        Self {
            scenario,
            qr_expires: 0.5,
            hang_secs: 30.0,
            dir,
        }
    }

    fn qr_expires(mut self, secs: f64) -> Self {
        self.qr_expires = secs;
        self
    }

    fn script() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("fake_whatsapp_bridge.py")
    }
}

impl BridgeLauncher for FixtureLauncher {
    fn command(&self) -> Result<Command, BridgeError> {
        let mut cmd = Command::new("python3");
        cmd.arg(Self::script())
            .arg("--scenario")
            .arg(self.scenario)
            .arg("--qr-expires")
            .arg(self.qr_expires.to_string())
            .arg("--hang-secs")
            .arg(self.hang_secs.to_string())
            // A fixture assume defaults se o handshake nao chegar; o driver
            // sempre manda, entao um prazo generoso evita flake em CI lento.
            .arg("--handshake-timeout")
            .arg("5");
        Ok(cmd)
    }

    fn describe(&self) -> String {
        format!(
            "python3 fake_whatsapp_bridge.py --scenario {}",
            self.scenario
        )
    }

    fn dir(&self) -> PathBuf {
        self.dir.clone()
    }
}

/// UI que grava a sequencia, para os testes afirmarem o que o usuario viu.
#[derive(Default)]
struct RecordingUi {
    lines: Vec<String>,
}

impl PairUi for RecordingUi {
    fn status(&mut self, line: &str) {
        self.lines.push(format!("status:{line}"));
    }
    fn qr(&mut self, _data: &str, attempt: u32, max: u32, previous_expired: bool) {
        self.lines
            .push(format!("qr:{attempt}/{max}:expirou={previous_expired}"));
    }
    fn waiting(&mut self, attempt: u32, _max: u32, _left: u64) {
        self.lines.push(format!("waiting:{attempt}"));
    }
    fn authenticated(&mut self) {
        self.lines.push("authenticated".into());
    }
}

fn store_in(dir: &tempfile::TempDir) -> (SessionStore, SessionKey) {
    let store = SessionStore::for_data_dir(dir.path(), "default");
    let key = SessionKey::resolve(store.dir(), Some("passphrase-de-teste")).expect("chave");
    (store, key)
}

fn never_cancelled() -> watch::Receiver<bool> {
    let (tx, rx) = watch::channel(false);
    // O sender precisa viver enquanto o receiver existir, senao `changed()`
    // devolve erro e o driver entende como cancelamento.
    Box::leak(Box::new(tx));
    rx
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn pair_ok_persists_the_blob_and_reports_connected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);
    let launcher = FixtureLauncher::new("pair-ok", dir.path().to_path_buf());
    let mut ui = RecordingUi::default();

    let outcome = pair(&launcher, &store, &key, &mut ui, never_cancelled())
        .await
        .expect("pareamento");

    assert!(outcome.session_saved, "o blob precisa ter sido gravado");
    assert!(!outcome.reused_existing_session);
    // Os 4 ultimos digitos do JID da fixture (`5511999990000:1@…`).
    assert_eq!(outcome.phone_last4.as_deref(), Some("0000"));
    assert!(store.exists(), "session.enc precisa existir");

    // E precisa abrir com a mesma chave.
    let blob = store.load(&key).expect("load");
    assert!(!blob.is_empty());

    assert!(
        ui.lines.iter().any(|l| l.starts_with("qr:1/5")),
        "o usuario precisa ter visto o QR: {:?}",
        ui.lines
    );
    assert!(ui.lines.iter().any(|l| l == "authenticated"));
}

#[tokio::test]
async fn running_pair_twice_reuses_the_session_and_never_asks_for_a_qr() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    pair(
        &FixtureLauncher::new("pair-ok", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect("primeiro pareamento");

    // Com a sessao carregada, a fixture conecta direto — que e o que o bridge
    // real faz. A idempotencia esta em nao pedir QR, nao arquivar e nao apagar.
    let before = std::fs::read(store.blob_path()).expect("read");
    let mut ui = RecordingUi::default();
    let outcome = pair(
        &FixtureLauncher::new("session-ok", dir.path().to_path_buf()),
        &store,
        &key,
        &mut ui,
        never_cancelled(),
    )
    .await
    .expect("segundo pareamento");

    assert!(outcome.session_saved);
    assert!(
        outcome.reused_existing_session,
        "a segunda execucao precisa reaproveitar a sessao: {:?}",
        ui.lines
    );
    assert!(
        !ui.lines.iter().any(|l| l.starts_with("qr:")),
        "nenhum QR pode ter sido pedido: {:?}",
        ui.lines
    );
    assert!(store.exists(), "a sessao nao pode ter sumido");
    assert!(
        !store.archive_path().exists(),
        "nada foi arquivado: a sessao ainda vale"
    );
    let after = std::fs::read(store.blob_path()).expect("read");
    assert_ne!(
        before, after,
        "o blob e regravado (nonce novo), o que prova escrita atomica"
    );
}

#[tokio::test]
async fn an_expired_qr_is_replaced_and_the_second_attempt_is_announced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);
    let launcher =
        FixtureLauncher::new("pair-expire-then-ok", dir.path().to_path_buf()).qr_expires(2.5);
    let mut ui = RecordingUi::default();

    let outcome = pair(&launcher, &store, &key, &mut ui, never_cancelled())
        .await
        .expect("pareamento na segunda tentativa");

    assert!(outcome.session_saved);
    assert!(
        ui.lines.contains(&"qr:1/5:expirou=false".to_string()),
        "primeiro QR: {:?}",
        ui.lines
    );
    assert!(
        ui.lines.contains(&"qr:2/5:expirou=true".to_string()),
        "o segundo QR precisa ser anunciado como substituto: {:?}",
        ui.lines
    );
    assert!(
        ui.lines.iter().any(|l| l.starts_with("waiting:")),
        "o contador regressivo precisa ter aparecido: {:?}",
        ui.lines
    );
}

/// Depois de um re-link bem-sucedido o `session.enc.prev` nao pode sobreviver:
/// ele e material de autenticacao vivo num arquivo que ninguem mais abre.
#[tokio::test]
async fn a_successful_relink_discards_the_archived_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    // Pareia, arquiva (como faz o `link` quando o usuario aceita re-vincular)
    // e pareia de novo.
    pair(
        &FixtureLauncher::new("pair-ok", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect("primeiro pareamento");
    assert!(store.archive().expect("archive"));
    assert!(store.archive_path().is_file());

    pair(
        &FixtureLauncher::new("pair-ok", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect("re-link");

    assert!(store.exists(), "a sessao nova esta la");
    assert!(
        !store.archive_path().exists(),
        "a sessao arquivada precisa ter sido descartada depois do re-link"
    );
}

#[tokio::test]
async fn a_logged_out_account_purges_the_session_and_reports_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    // Sessao previa em disco, como quem ja tinha pareado.
    pair(
        &FixtureLauncher::new("pair-ok", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect("pareamento inicial");
    assert!(store.exists());

    let err = pair(
        &FixtureLauncher::new("logged-out", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect_err("logged_out precisa falhar");

    assert!(
        matches!(
            err,
            RunError::SessionDead {
                reason_code: Some(401)
            }
        ),
        "o codigo cru do Baileys precisa chegar ao usuario: {err:?}"
    );
    assert!(
        !store.exists(),
        "uma conta deslogada nao pode deixar o blob para tras"
    );
    assert!(!store.key_path().exists(), "a chave tambem some");
}

#[tokio::test]
async fn a_crash_after_the_qr_surfaces_a_clear_error_and_persists_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    let err = pair(
        &FixtureLauncher::new("crash-after-qr", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect_err("o bridge morreu");

    let msg = err.to_string();
    assert!(
        msg.contains("bridge"),
        "a mensagem precisa dizer o que morreu: {msg}"
    );
    assert!(!store.exists(), "nada pode ter sido gravado");
    assert!(!store.archive_path().exists());
}

#[tokio::test]
async fn a_protocol_version_mismatch_is_refused_with_the_bridge_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    let err = pair(
        &FixtureLauncher::new("bad-protocol", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect_err("protocolo incompativel");

    let msg = err.to_string();
    assert!(
        msg.contains("99"),
        "precisa citar a versao encontrada: {msg}"
    );
    assert!(
        msg.contains(&dir.path().display().to_string()),
        "precisa citar o diretorio do bridge para o usuario apagar: {msg}"
    );
}

#[tokio::test]
async fn free_text_on_stdout_is_a_protocol_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    let err = pair(
        &FixtureLauncher::new("garbage", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect_err("texto livre no stdout");
    assert!(err.to_string().contains("protocolo"), "{err}");
}

#[tokio::test]
async fn an_oversized_line_is_rejected_instead_of_allocated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    let err = pair(
        &FixtureLauncher::new("oversized", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect_err("linha grande demais");

    let msg = err.to_string();
    assert!(msg.contains("teto"), "{msg}");
    // A mensagem de erro nunca pode carregar o conteudo da linha.
    assert!(!msg.contains("xxxxxxxxxx"), "a linha vazou no erro: {msg}");
}

/// O `pair` da ponte **nao tem prazo proprio** — ele reconecta para sempre.
/// Quem cronometra e este lado, e este teste e a prova: com o bridge mudo, o
/// driver desiste sozinho, com mensagem, sem persistir nada e sem depender de
/// um `timeout` externo.
#[tokio::test]
async fn a_silent_bridge_makes_the_driver_give_up_on_its_own() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    let outer = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        pair_with(
            &FixtureLauncher::new("hang", dir.path().to_path_buf()),
            &store,
            &key,
            &mut SilentUi,
            never_cancelled(),
            PairOptions {
                stall_after_secs: 3,
            },
        ),
    )
    .await
    .expect("o driver precisa desistir SOZINHO, sem o timeout externo");

    let err = outer.expect_err("bridge mudo");
    let msg = err.to_string();
    assert!(
        msg.contains("parou de responder"),
        "a mensagem precisa dizer o que aconteceu: {msg}"
    );
    assert!(!store.exists(), "nada pode ter sido gravado");
}

/// O prazo de silencio nao pode disparar num pareamento que esta progredindo:
/// cada evento do bridge zera o contador.
#[tokio::test]
async fn a_slow_but_progressing_pairing_is_not_cut_short() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    // Dois QRs com 2 s de validade cada, e um prazo de silencio de 3 s: o
    // pareamento leva mais que o prazo, mas nunca fica 3 s calado.
    let outcome = pair_with(
        &FixtureLauncher::new("pair-expire-then-ok", dir.path().to_path_buf()).qr_expires(2.0),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
        PairOptions {
            stall_after_secs: 3,
        },
    )
    .await
    .expect("o pareamento lento precisa concluir");

    assert!(outcome.session_saved);
}

#[tokio::test]
async fn ctrl_c_cancels_without_persisting_anything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);
    let (tx, rx) = watch::channel(false);

    let handle = {
        let launcher = FixtureLauncher::new("hang", dir.path().to_path_buf());
        let store = store.clone();
        tokio::spawn(async move { pair(&launcher, &store, &key, &mut SilentUi, rx).await })
    };

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    tx.send(true).expect("cancelar");

    let err = handle.await.expect("join").expect_err("cancelado");
    assert!(matches!(err, RunError::Cancelled), "veio {err:?}");
    assert!(!store.exists(), "Ctrl+C nao persiste nada");
}

// ---------------------------------------------------------------------------
// Modo serve — a costura do gateway
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CollectingSink {
    messages: Mutex<Vec<InboundMessage>>,
    connections: Mutex<Vec<bool>>,
}

impl InboundSink for CollectingSink {
    fn deliver(&self, message: InboundMessage) {
        if let Ok(mut guard) = self.messages.lock() {
            guard.push(message);
        }
    }
    fn on_connection(&self, _jid: Option<&Jid>, connected: bool) {
        if let Ok(mut guard) = self.connections.lock() {
            guard.push(connected);
        }
    }
}

#[tokio::test]
async fn serve_delivers_messages_and_refreshes_the_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);
    pair(
        &FixtureLauncher::new("pair-ok", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect("pareamento");

    let sink = Arc::new(CollectingSink::default());
    let (tx, rx) = watch::channel(false);
    let (out_tx, out_rx) = tokio::sync::mpsc::channel(4);
    let launcher: Arc<dyn BridgeLauncher> =
        Arc::new(FixtureLauncher::new("serve-echo", dir.path().to_path_buf()));

    let task = {
        let sink = Arc::clone(&sink);
        let store = store.clone();
        tokio::spawn(async move { serve(launcher, store, key, sink, out_rx, rx, || 0.0).await })
    };

    // O caminho de saida: e por este canal que o gateway respondera.
    out_tx
        .send(BridgeCommand::Send {
            request_id: "r1".into(),
            chat_jid: Jid::new("5511888880000@s.whatsapp.net"),
            text: "pong".into(),
        })
        .await
        .expect("enfileirar send");

    // Espera o eco chegar sem `sleep` cego demais.
    for _ in 0..60 {
        if sink.messages.lock().is_ok_and(|m| !m.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    tx.send(true).expect("cancelar");
    task.await.expect("join").expect("serve encerra limpo");

    let messages = sink.messages.lock().expect("lock");
    assert_eq!(messages.len(), 1, "o eco da fixture precisa chegar");
    assert_eq!(
        messages[0].text.as_deref(),
        Some("echo: pong"),
        "a fixture ecoa o texto que saiu pelo canal de comandos"
    );
    assert!(store.exists(), "a sessao continua la depois do serve");
}

#[tokio::test]
async fn serve_reconnects_after_a_network_flap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);
    pair(
        &FixtureLauncher::new("pair-ok", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect("pareamento");

    let sink = Arc::new(CollectingSink::default());
    let (tx, rx) = watch::channel(false);
    // `network-flap` conecta, cai e sai 0 a cada execucao: cada reconexao do
    // driver produz mais um `on_connection(true)`. Com jitter 0 o primeiro
    // degrau e 500 ms, entao 2 s cobrem varias voltas.
    let launcher: Arc<dyn BridgeLauncher> = Arc::new(FixtureLauncher::new(
        "network-flap",
        dir.path().to_path_buf(),
    ));

    let (_out_tx, out_rx) = tokio::sync::mpsc::channel(1);
    let task = {
        let sink = Arc::clone(&sink);
        let store = store.clone();
        tokio::spawn(async move { serve(launcher, store, key, sink, out_rx, rx, || 0.0).await })
    };

    tokio::time::sleep(std::time::Duration::from_millis(2_000)).await;
    tx.send(true).expect("cancelar");
    let _ = task.await.expect("join");

    let connects = sink
        .connections
        .lock()
        .expect("lock")
        .iter()
        .filter(|c| **c)
        .count();
    assert!(
        connects >= 2,
        "o driver precisa ter reconectado ao menos uma vez (viu {connects})"
    );
}

#[tokio::test]
async fn serve_stops_and_purges_when_the_account_is_logged_out() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);
    pair(
        &FixtureLauncher::new("pair-ok", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect("pareamento");

    let sink = Arc::new(CollectingSink::default());
    let (_tx, rx) = watch::channel(false);
    let launcher: Arc<dyn BridgeLauncher> =
        Arc::new(FixtureLauncher::new("logged-out", dir.path().to_path_buf()));

    let (_out_tx, out_rx) = tokio::sync::mpsc::channel(1);
    let err = serve(launcher, store.clone(), key, sink, out_rx, rx, || 0.0)
        .await
        .expect_err("logged out");
    assert!(matches!(err, RunError::SessionDead { .. }), "veio {err:?}");
    assert!(!store.exists(), "o material some quando a sessao morre");
}
