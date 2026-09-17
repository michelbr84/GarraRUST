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
    BridgeCommand, InboundMessage, Jid, SessionKey, SessionStore, backoff_ms,
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
    let store = SessionStore::for_data_dir(dir.path(), "default").expect("conta valida");
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
    // Nota: com passphrase o `session.key` nunca chega a existir, entao a
    // assercao sobre ele e vacua nesta fixture. O que prova a limpeza aqui e
    // o SALT: ele existia e tem de sumir junto, porque nao ha arquivado a
    // proteger.
    assert!(!store.key_path().exists(), "a chave tambem some");
    assert!(
        !store.salt_path().exists(),
        "e o salt vai junto quando nao ha nada arquivado a preservar"
    );
}

/// A sequencia que o `purge` do `pair` destruia em silencio: o usuario aceita
/// re-vincular, o `link` ARQUIVA a sessao boa, o pareamento novo e recusado
/// pela Meta (401) — e o arquivado tem de continuar la, com a chave e o salt,
/// para o guard do chamador poder devolve-lo.
///
/// Antes: `purge()` triturava blob, `.prev`, `session.key` e `session.salt`,
/// entao o `Drop` do guard nao achava nada para restaurar e caia no braco
/// `Ok(false)`, que era silencioso. O usuario perdia o vinculo anterior e so
/// lia "esta sessao nao vale mais".
#[tokio::test]
async fn a_relink_refused_by_the_server_never_destroys_the_archived_session() {
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
    .expect("pareamento inicial");
    let anterior = store.load(&key).expect("a sessao boa abre");

    // Exatamente o que o `ArchiveGuard` do `link` faz ao aceitar o re-vinculo.
    assert!(store.archive().expect("archive"));

    let err = pair(
        &FixtureLauncher::new("logged-out", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
    )
    .await
    .expect_err("a Meta recusou o vinculo novo");
    assert!(
        matches!(err, RunError::SessionDead { .. }),
        "o desfecho continua sendo sessao morta: {err:?}"
    );

    assert!(
        store.archive_path().is_file(),
        "o arquivado e a sessao do USUARIO, nao material deste pareamento"
    );
    // `store_in` deriva a chave de uma passphrase, entao o que precisa
    // sobreviver aqui e o SALT: sem ele a derivacao muda e o arquivado deixa
    // de abrir. No modo sem passphrase quem sobrevive e o `session.key`, e
    // esse caso esta pinado em `session.rs`
    // (`a_dead_session_never_takes_the_archived_one_nor_its_key`).
    assert!(
        store.salt_path().exists(),
        "sem o salt o arquivado nao recupera nada — .prev, key e salt vivem ou morrem juntos"
    );
    assert!(
        store.restore_archive().expect("restore"),
        "e o guard consegue devolve-lo"
    );
    assert_eq!(
        store.load(&key).expect("a sessao restaurada abre"),
        anterior,
        "e o que volta e a MESMA sessao"
    );
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
                ..PairOptions::default()
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

/// **O handshake tem prazo, e o Ctrl+C alcanca ele.**
///
/// Ate a revisao R4 tudo o que vinha antes do `tokio::select!` do laco —
/// `expect_started` e os dois `send` — rodava sem relogio e sem cancelamento:
/// o ticker, o watchdog de silencio e o braco de `cancel` so comecam depois.
/// Um `node` que sobe e nunca fala (shim de asdf/volta/nvm, stub de snap)
/// pendurava o terminal para sempre, e nem o primeiro nem o segundo Ctrl+C
/// faziam nada, porque a CLI ja tinha trocado o SIGINT default do sistema por
/// um canal que ninguem estava lendo.
///
/// Nenhum cenario cobria isso porque **todos** emitem `started` antes de
/// qualquer outra coisa — inclusive o `hang` e o `garbage`. Dai o
/// `silent-start`.
///
/// O `timeout` externo aqui e rede de seguranca do teste, nao o mecanismo: se
/// ele for quem dispara, o driver falhou.
#[tokio::test]
async fn a_bridge_that_never_says_started_does_not_hang_the_terminal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    let outer = tokio::time::timeout(
        std::time::Duration::from_secs(12),
        pair_with(
            &FixtureLauncher::new("silent-start", dir.path().to_path_buf()),
            &store,
            &key,
            &mut SilentUi,
            never_cancelled(),
            PairOptions {
                stall_after_secs: 3,
                ..PairOptions::default()
            },
        ),
    )
    .await
    .expect("o driver ficou pendurado no handshake: nenhum prazo o alcanca");

    let err = outer.expect_err("um bridge mudo no handshake nao pode virar sucesso");
    let msg = err.to_string();
    // Destravar nao basta: a mensagem tem de dizer o que houve e o que fazer.
    assert!(
        msg.contains("nao respondeu o handshake"),
        "a mensagem precisa nomear o handshake: {msg}"
    );
    assert!(
        msg.contains("node --version"),
        "e precisa dizer o que fazer a seguir: {msg}"
    );
    assert!(!store.exists(), "nada pode ter sido gravado");
}

/// E o Ctrl+C volta a matar o processo **durante** o handshake.
///
/// O prazo sozinho nao resolveria o que o dono nomeou como inaceitavel: 90 s
/// de tela parada ainda sao 90 s. O braco de cancelamento e a outra metade, e
/// ele vem `biased` para nunca ficar atras do relogio.
#[tokio::test]
async fn ctrl_c_during_the_handshake_is_not_swallowed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);
    let (tx, rx) = watch::channel(false);

    let handle = {
        let launcher = FixtureLauncher::new("silent-start", dir.path().to_path_buf());
        let store = store.clone();
        tokio::spawn(async move {
            pair_with(
                &launcher,
                &store,
                &key,
                &mut SilentUi,
                rx,
                PairOptions {
                    // Prazo folgado de proposito: quem tem de terminar este
                    // teste e o Ctrl+C, nao o relogio.
                    stall_after_secs: 600,
                    ..PairOptions::default()
                },
            )
            .await
        })
    };

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    tx.send(true).expect("cancelar");

    let err = tokio::time::timeout(std::time::Duration::from_secs(12), handle)
        .await
        .expect("o Ctrl+C precisa alcancar o handshake")
        .expect("join")
        .expect_err("cancelado");
    assert!(matches!(err, RunError::Cancelled), "veio {err:?}");
    assert!(!store.exists(), "Ctrl+C no handshake nao persiste nada");
}

/// O prazo de silencio nao pode disparar num pareamento que esta progredindo:
/// cada evento do bridge zera o contador.
#[tokio::test]
async fn a_slow_but_progressing_pairing_is_not_cut_short() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    // Dois QRs de 4 s e um prazo de silencio de 7 s: o pareamento inteiro
    // (~8 s) passa do prazo, mas o maior silencio (~4 s) fica bem abaixo dele.
    //
    // A folga e o ponto. Com 2 s de QR contra 3 s de prazo bastava a fixture
    // Python levar 50% a mais que o pedido — rotineiro num `cargo test`
    // paralelo numa maquina carregada — para o watchdog disparar num
    // pareamento saudavel, e o teste piscava. Agora e preciso 75%. O relogio
    // continua sendo de parede: o driver so tem `stall_after_secs` como
    // costura, e encurtar o prazo e justamente o que o teste precisa fazer.
    let outcome = pair_with(
        &FixtureLauncher::new("pair-expire-then-ok", dir.path().to_path_buf()).qr_expires(4.0),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
        PairOptions {
            stall_after_secs: 7,
            ..PairOptions::default()
        },
    )
    .await
    .expect("o pareamento lento precisa concluir");

    assert!(outcome.session_saved);
}

/// O guarda de silencio exclui `Phase::Connected` de proposito — conectado, o
/// driver espera o `session_update` final. So que "espera" sem teto e o
/// terminal pendurado para sempre: uma ponte que conecta e nunca fecha o
/// stdout nao tem prazo nenhum, nem do lado dela nem daqui.
///
/// Estourar o teto NAO e erro: o que ja chegou tem de estar gravado.
#[tokio::test]
async fn a_bridge_that_connects_and_then_goes_quiet_is_not_waited_on_forever() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        pair_with(
            &FixtureLauncher::new("connect-then-hang", dir.path().to_path_buf()),
            &store,
            &key,
            &mut SilentUi,
            never_cancelled(),
            PairOptions {
                // Prazo de pareamento folgado: o unico teto que pode disparar
                // neste cenario e o de flush final.
                stall_after_secs: 90,
                final_flush_secs: 3,
                ..PairOptions::default()
            },
        ),
    )
    .await
    .expect("o driver precisa fechar SOZINHO depois de conectar, sem o timeout externo");

    let outcome = outcome.expect("a sessao chegou antes do silencio: isto nao e falha");
    assert!(
        outcome.session_saved,
        "o blob que a ponte entregou antes de emudecer tem de ser gravado"
    );
    assert!(store.exists());
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
    // `network-flap` em modo serve conecta, cai, reconecta dentro da propria
    // execucao e entao SAI 0 — a ponte morre. Cada execucao produz dois
    // `on_connection(true)`, e so uma reconexao do DRIVER produz o terceiro.
    // Com jitter 0 o primeiro degrau e 500 ms, e a execucao da fixture com o
    // QR curto abaixo leva ~0,15 s, entao 3 s cobrem varias voltas com folga.
    //
    // O QR da fixture expira rapido de proposito: a asserção de tempo la
    // embaixo so distingue "esperou o backoff" de "reconectou em busy-loop"
    // enquanto UMA execucao da fixture custar bem menos que o backoff. Com o
    // default de 0,5 s a execucao sozinha ja passava dos 500 ms do primeiro
    // degrau, e a asserção passaria verde com o `sleep` arrancado.
    let launcher: Arc<dyn BridgeLauncher> =
        Arc::new(FixtureLauncher::new("network-flap", dir.path().to_path_buf()).qr_expires(0.02));

    // O `serve` nao usa a maquina de estados: o backoff entre tentativas e
    // dele. Marca-se o INSTANTE de cada chamada de `jitter`, e nao o numero
    // delas: contar prova que o atraso foi calculado, nao que alguem esperou —
    // com o `tokio::select!{ sleep(delay) }` arrancado, o contador continuava
    // subindo e o teste continuava verde em cima de um busy-loop de
    // reconexao.
    let jitter_marks: Arc<Mutex<Vec<std::time::Instant>>> = Arc::new(Mutex::new(Vec::new()));

    let (_out_tx, out_rx) = tokio::sync::mpsc::channel(1);
    let task = {
        let sink = Arc::clone(&sink);
        let store = store.clone();
        let jitter_marks = Arc::clone(&jitter_marks);
        tokio::spawn(async move {
            serve(launcher, store, key, sink, out_rx, rx, move || {
                if let Ok(mut marks) = jitter_marks.lock() {
                    marks.push(std::time::Instant::now());
                }
                0.0
            })
            .await
        })
    };

    tokio::time::sleep(std::time::Duration::from_millis(3_000)).await;
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
        connects >= 3,
        "duas conexoes saem de uma execucao so da fixture; a terceira e a \
primeira que exige o driver ter relancado a ponte (viu {connects})"
    );
    let marks = jitter_marks.lock().expect("lock").clone();
    assert!(
        marks.len() >= 2,
        "sao precisas duas reconexoes para medir um intervalo; vieram {}",
        marks.len()
    );
    let esperado = std::time::Duration::from_millis(backoff_ms(1, 0.0));
    let medido = marks[1].duration_since(marks[0]);
    assert!(
        medido >= esperado,
        "entre duas reconexoes passaram {medido:?}, menos que o primeiro \
degrau do backoff ({esperado:?}): o `serve` calculou o atraso e nao esperou \
por ele — isto e o busy-loop de reconexao"
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

/// **"Connecting… para sempre", pelo caminho que o watchdog de silencio nao
/// alcanca.**
///
/// O prazo de silencio da rodada 4 conta desde o ULTIMO evento, e zera a
/// qualquer um deles — inclusive os que significam "falhei de novo". A ponte
/// real reconecta sozinha, sem teto, emitindo `disconnected{will_retry:true}`
/// + `status` a cada rodada de backoff (≤ 30 s, sempre abaixo dos 90 s do
/// watchdog). Resultado: o watchdog existe, funciona, e nunca dispara, porque
/// o proprio fracasso o realimenta.
///
/// E o usuario sem internet, atras de captive portal, com 443 bloqueado ou com
/// o relogio errado — nao um caso de laboratorio. A tela parava em duas linhas
/// e nao andava mais; so o Ctrl+C saia.
///
/// As duas metades do conserto estao asseridas aqui, e cada uma sozinha
/// deixaria metade do problema em pe:
/// 1. a queda **aparece** na tela, com motivo e prazo;
/// 2. o "nunca progrediu" tem teto, e o relogio dele **nao zera com evento**.
#[tokio::test]
async fn a_bridge_that_retries_forever_is_neither_silent_nor_endless() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);
    let mut ui = RecordingUi::default();

    let outer = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        pair_with(
            &FixtureLauncher::new("retry-forever", dir.path().to_path_buf()),
            &store,
            &key,
            &mut ui,
            never_cancelled(),
            PairOptions {
                // O prazo de SILENCIO e curto de proposito: mesmo curto, ele
                // nao dispara, porque a ponte fala a cada segundo. Quem tem de
                // cortar e o outro.
                stall_after_secs: 3,
                final_flush_secs: 3,
                no_progress_after_secs: 6,
            },
        ),
    )
    .await
    .expect("o driver precisa desistir SOZINHO — o timeout externo nao e o mecanismo");

    let err = outer.expect_err("nunca houve QR nem conexao");
    let msg = err.to_string();
    assert!(
        msg.contains("sem chegar a um QR nem conectar"),
        "a mensagem precisa dizer que nao houve progresso: {msg}"
    );
    assert!(
        msg.contains("garra whatsapp"),
        "e precisa ser acionavel — dizer o que fazer: {msg}"
    );

    // Metade 1: a tela andou. Sem isso o usuario passa os 6 s (120 s em
    // producao) olhando exatamente as mesmas duas linhas.
    let quedas = ui
        .lines
        .iter()
        .filter(|l| l.starts_with("status:") && l.contains("nova tentativa em"))
        .count();
    assert!(
        quedas >= 2,
        "cada tentativa fracassada tem de aparecer na tela, com motivo e prazo; \
o usuario viu: {:?}",
        ui.lines
    );
    assert!(
        ui.lines.iter().any(|l| l.contains("a rede caiu")),
        "a linha precisa dizer o MOTIVO que a ponte reportou: {:?}",
        ui.lines
    );

    assert!(!store.exists(), "nada pode ter sido gravado");
}

/// O prazo de "nunca progrediu" **nao** pode cortar um pareamento que
/// progrediu e depois ficou esperando a leitura do QR.
///
/// Sem esta metade o conserto acima seria um teto burro sobre o fluxo normal:
/// um usuario que demora a pegar o celular veria o comando desistir no meio.
#[tokio::test]
async fn a_pairing_that_showed_a_qr_is_never_cut_by_the_no_progress_deadline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);
    let mut ui = RecordingUi::default();

    // O prazo e 1 s — menor que o proprio pareamento. Se ele contasse o
    // tempo total em vez de "tempo sem progresso", este teste falharia.
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        pair_with(
            &FixtureLauncher::new("pair-expire-then-ok", dir.path().to_path_buf()).qr_expires(1.5),
            &store,
            &key,
            &mut ui,
            never_cancelled(),
            PairOptions {
                stall_after_secs: 30,
                final_flush_secs: 5,
                no_progress_after_secs: 1,
            },
        ),
    )
    .await
    .expect("o pareamento nao pode pendurar")
    .expect("um pareamento que mostrou QR e conectou nao pode ser cortado pelo prazo");

    assert!(outcome.session_saved);
    assert!(
        ui.lines.iter().any(|l| l.starts_with("qr:")),
        "este cenario existe para mostrar QR: {:?}",
        ui.lines
    );
}

/// **A redacao do stderr esta ligada NO CALL SITE, e nao so testada como
/// funcao pura.**
///
/// `redact_tail_line` tinha teste; `stderr_hint` nao tinha nenhum. Neutralizar
/// os dois call sites — devolver a linha crua em vez da redigida — deixava
/// **todos** os testes verdes com a correcao de seguranca inteira desfeita.
/// Este teste roda o processo de verdade e olha o que chega a tela.
#[tokio::test]
async fn the_error_shown_to_the_user_never_carries_the_bridge_credential() {
    // A mesma constante da fixture: base64 padrao de 32 B, a forma de uma
    // `noiseKey` do Baileys.
    const SECRET: &str = "c2VjcmV0/Y3JlZGVudGlhbCtub2lzZUtleUJBU0U2ND0=";

    let dir = tempfile::tempdir().expect("tempdir");
    let (store, key) = store_in(&dir);

    let err = pair_with(
        &FixtureLauncher::new("crash-with-secret", dir.path().to_path_buf()),
        &store,
        &key,
        &mut SilentUi,
        never_cancelled(),
        PairOptions {
            stall_after_secs: 10,
            final_flush_secs: 3,
            ..PairOptions::default()
        },
    )
    .await
    .expect_err("a ponte morreu antes de conectar");

    let msg = err.to_string();
    assert!(
        msg.contains("Ultimas linhas do bridge:"),
        "a cauda do stderr precisa chegar ao usuario — e ela que este teste vigia: {msg}"
    );
    assert!(
        !msg.contains(SECRET),
        "o material de credencial chegou a tela CRU:\n{msg}"
    );
    assert!(
        msg.contains("<redigido:"),
        "a sequencia longa tinha de ter sido redigida: {msg}"
    );
    // E a redacao nao pode custar o diagnostico: o caminho do modulo que o
    // Node citou continua legivel, que e o motivo de a cauda existir.
    assert!(
        msg.contains("@whiskeysockets/baileys/lib/index.js"),
        "o caminho do modulo tem de continuar legivel: {msg}"
    );
}

/// O **segundo** call site da redacao: a cauda do `npm ci` que falhou.
///
/// Os dois call sites (`stderr_hint` e `npm_ci`) precisam de teste separado —
/// neutralizar so um deixaria o outro verde, que e exatamente o modo de falha
/// que esta rodada encontrou.
#[tokio::test]
async fn a_failed_npm_ci_never_shows_a_credential_from_its_stderr() {
    const SECRET: &str = "c2VjcmV0/Y3JlZGVudGlhbCtub2lzZUtleUJBU0U2ND0=";

    let dir = tempfile::tempdir().expect("tempdir");
    let fake_npm = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fake_npm.py");

    let err = garraia_channels::whatsapp_linked::bridge::npm_ci(&fake_npm, dir.path())
        .await
        .expect_err("o npm falso sai 1");

    let msg = err.to_string();
    assert!(
        msg.contains("npm ERR! code ERESOLVE"),
        "a cauda precisa chegar ao usuario: {msg}"
    );
    assert!(
        !msg.contains(SECRET),
        "o `_auth` do npm chegou a tela CRU:\n{msg}"
    );
    assert!(
        msg.contains("<redigido:"),
        "a sequencia longa tinha de ter sido redigida: {msg}"
    );
    assert!(
        msg.contains("@whiskeysockets/baileys/package.json"),
        "o caminho tem de continuar legivel: {msg}"
    );
}
