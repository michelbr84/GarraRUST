//! Drivers que casam [`super::bridge`], [`super::state`] e [`super::session`].
//!
//! Sao dois, e a diferenca entre eles e o motivo de existir um modulo a mais do
//! que o plano listava:
//!
//! - [`pair`] roda uma vez, no terminal, com o usuario olhando. Termina.
//! - [`serve`] e a costura do slice do gateway: longa duracao, reconecta com
//!   backoff e entrega mensagem num [`InboundSink`].
//!
//! Os dois compartilham o enquadramento, a maquina de estados e o store; o que
//! os separa e quem consome os eventos. Deixa-los na CLI teria posto o loop de
//! reconexao — a parte que o gateway precisa — no unico lugar onde o gateway
//! nao pode chegar.
//!
//! # Onde o tempo entra
//!
//! Em lugar nenhum da maquina de estados. Aqui, e so aqui, existe relogio: um
//! `tokio::time::interval` e um braco do `tokio::select!`, no mesmo padrao do
//! `spinner.rs` da CLI — **nunca uma task propria**. Isso e o que permite o
//! contador regressivo do QR sem uma segunda thread competindo pelo terminal.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use super::bridge::{BridgeConnection, BridgeError, BridgeLauncher};
use super::protocol::{
    BridgeCommand, BridgeEvent, BridgeState, DisconnectReason, ErrorCode, InboundMessage, Jid,
    StartMode,
};
use super::session::{SessionBlob, SessionError, SessionKey, SessionStore};
use super::state::{Desired, Effect, Event, Failure, Machine, Phase, backoff_ms};

/// Periodo do braco de relogio. Um segundo e o passo do contador regressivo
/// que o usuario ve; a maquina de estados so precisa de granularidade de
/// expiracao de QR, que e de 20 s.
const TICK: Duration = Duration::from_secs(1);

/// Falhas do driver.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Bridge(#[from] BridgeError),

    #[error(transparent)]
    Session(#[from] SessionError),

    /// Os 5 QRs expiraram sem leitura.
    #[error("nenhum QR foi lido")]
    QrExpired,

    /// O usuario cancelou (Ctrl+C). Nada foi persistido.
    #[error("cancelado")]
    Cancelled,

    /// A sessao morreu (401/403/419). Um QR novo passa a ser obrigatorio.
    ///
    /// `reason_code` e o codigo cru do Baileys, quando ele veio: ele e a
    /// diferenca entre "voce desconectou o aparelho no celular" (401) e
    /// "a Meta recusou esta sessao" (403/419), e o usuario merece ver qual foi.
    #[error("a sessao nao vale mais (codigo {reason_code:?})")]
    SessionDead { reason_code: Option<i64> },

    /// O bridge subiu sem `node_modules`: o handshake funciona, mas nada alem
    /// dele. Erro proprio para a CLI mandar rodar `npm ci` no diretorio certo.
    #[error("o bridge esta sem dependencias instaladas — rode `npm ci` em {dir}")]
    MissingDependencies { dir: String },
}

/// Como o driver conversa com a tela.
///
/// Trait e nao `println!` porque o teste precisa afirmar **a sequencia** de
/// mensagens (tentativa 2 apareceu? o QR anterior foi anunciado como expirado?)
/// sem capturar stdout do processo.
pub trait PairUi: Send {
    /// Linha curta de status (`→ conectando…`).
    fn status(&mut self, line: &str);
    /// Um QR novo chegou. `previous_expired` diz se ele substitui um anterior.
    fn qr(&mut self, data: &str, attempt: u32, max_attempts: u32, previous_expired: bool);
    /// Contador regressivo enquanto esperamos a leitura.
    fn waiting(&mut self, attempt: u32, max_attempts: u32, seconds_left: u64);
    /// O pareamento foi aceito e a sessao esta sincronizando.
    fn authenticated(&mut self);
}

/// UI muda, para testes que so olham o resultado.
#[derive(Debug, Default)]
pub struct SilentUi;

impl PairUi for SilentUi {
    fn status(&mut self, _line: &str) {}
    fn qr(&mut self, _d: &str, _a: u32, _m: u32, _p: bool) {}
    fn waiting(&mut self, _a: u32, _m: u32, _s: u64) {}
    fn authenticated(&mut self) {}
}

/// Resultado de um pareamento bem-sucedido.
#[derive(Debug)]
pub struct PairOutcome {
    /// `true` quando havia sessao valida e nao foi preciso ler QR nenhum.
    pub reused_existing_session: bool,
    /// `true` quando um blob novo foi gravado em disco.
    pub session_saved: bool,
    /// Ultimos 4 digitos do proprio numero, quando o bridge reportou.
    pub phone_last4: Option<String>,
}

/// Fluxo de pareamento: `session_load` → `start{pair}` → QR → `connected`.
///
/// **A ordem de persistencia e o contrato**: o blob e gravado aqui, e so
/// depois o chamador grava `enabled = true` na config. E a regra do Hermes
/// (`creds.json` antes de `WHATSAPP_ENABLED`), e existe porque um wizard
/// abortado que deixou `enabled = true` faz o gateway pagar timeout e retry
/// infinitos a cada boot. Por isso [`pair`] nunca toca a config: quem a toca e
/// a CLI, depois de ver `session_saved`.
///
/// `cancel` e um `watch` em vez de um `CancellationToken` porque `tokio-util`
/// nao esta na arvore e um canal de um bit nao justifica traze-lo.
pub async fn pair(
    launcher: &dyn BridgeLauncher,
    store: &SessionStore,
    key: &SessionKey,
    ui: &mut dyn PairUi,
    mut cancel: watch::Receiver<bool>,
) -> Result<PairOutcome, RunError> {
    let mut machine = Machine::new();

    // 1. O disco decide o ponto de partida.
    let existing = if store.exists() {
        machine.on(Event::SessionFound, 0);
        match store.load(key) {
            Ok(blob) => Some(blob),
            Err(SessionError::Undecryptable) => {
                // A chave mudou (passphrase nova, `session.key` perdida). Nao
                // da para validar nem para apagar em silencio: arquiva e pareia
                // de novo, exatamente como numa sessao recusada pelo servidor.
                ui.status("sessao encontrada mas ilegivel com a chave atual — arquivando");
                store.archive()?;
                machine = Machine::new();
                machine.on(Event::NoSession, 0);
                None
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        machine.on(Event::NoSession, 0);
        None
    };
    let had_existing = existing.is_some();

    // 2. Sobe o bridge e confere o protocolo ANTES de mandar qualquer coisa.
    let mut conn = BridgeConnection::spawn(launcher).await?;
    let started = conn.expect_started().await?;
    if let BridgeEvent::Started {
        baileys_version: None,
        ..
    } = started
    {
        // O bridge responde o handshake sem `node_modules` de proposito (e o
        // que faz o `--protocol-check` funcionar numa arvore limpa). Parar
        // aqui, com o caminho certo na mensagem, e melhor do que deixar o
        // `start` falhar de um jeito que nao diz o que fazer.
        conn.kill().await;
        return Err(RunError::MissingDependencies {
            dir: launcher.dir().display().to_string(),
        });
    }
    machine.on(Event::BridgeStarted, 0);

    conn.send(&BridgeCommand::SessionLoad { session: existing })
        .await?;
    conn.send(&BridgeCommand::Start {
        mode: StartMode::Pair,
    })
    .await?;
    ui.status(if had_existing {
        "validando a sessao existente…"
    } else {
        "conectando ao WhatsApp…"
    });

    let mut now: u64 = 0;
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await; // o primeiro tick e imediato

    let mut latest_blob: Option<SessionBlob> = None;
    let mut phone_last4: Option<String> = None;
    let mut previous_expired = false;
    let mut pending_qr: Option<String> = None;
    let mut saw_connected = false;
    // `logged_out` sozinho nao prova nada: ele tambem sai num logout pedido e
    // num 440. Guardamos o sinal e deixamos o CODIGO DE SAIDA decidir se a
    // sessao morreu — e so entao se apaga alguma coisa.
    let mut saw_logged_out = false;
    let mut dead_reason_code: Option<i64> = None;

    loop {
        tokio::select! {
            // Cancelamento vence: Ctrl+C nao pode ficar atras de um evento.
            biased;

            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    machine.on(Event::Stop, now);
                    // Pede saida limpa: `shutdown` fecha o socket SEM deslogar,
                    // entao uma sessao ja valida sobrevive ao cancelamento.
                    let _ = conn.send(&BridgeCommand::Shutdown).await;
                    conn.kill().await;
                    return Err(RunError::Cancelled);
                }
            }

            _ = ticker.tick() => {
                now += 1;
                for effect in machine.tick(now) {
                    match effect {
                        Effect::QrExpired { .. } => previous_expired = true,
                        Effect::Fail(Failure::QrExpired) => {
                            let _ = conn.send(&BridgeCommand::Shutdown).await;
                            conn.kill().await;
                            return Err(RunError::QrExpired);
                        }
                        _ => {}
                    }
                }
                if let Phase::QrGenerated { attempt, expires_at_secs }
                    | Phase::WaitingScan { attempt, expires_at_secs } = machine.phase()
                {
                    ui.waiting(
                        attempt,
                        super::state::MAX_QR_ATTEMPTS,
                        expires_at_secs.saturating_sub(now),
                    );
                }
            }

            event = conn.next_event() => {
                let Some(event) = event? else {
                    // stdout fechou: o filho esta saindo.
                    machine.on(Event::BridgeExited, now);
                    break;
                };

                // O QR precisa ser guardado antes de aplicar o evento: so
                // depois da maquina sabemos QUAL tentativa ele e.
                if let BridgeEvent::Qr { ref data, .. } = event {
                    pending_qr = Some(data.clone());
                }
                if let BridgeEvent::SessionUpdate { ref session, .. } = event {
                    latest_blob = Some(session.clone());
                }
                if let BridgeEvent::Connected {
                    phone_last4: ref last4,
                    ..
                } = event
                {
                    phone_last4.clone_from(last4);
                    saw_connected = true;
                }
                if let BridgeEvent::Disconnected {
                    reason: DisconnectReason::LoggedOut,
                    reason_code,
                    ..
                } = event
                {
                    dead_reason_code = reason_code;
                }
                if matches!(event, BridgeEvent::LoggedOut) {
                    saw_logged_out = true;
                    // Nao apaga nada agora: espera o processo terminar.
                    continue;
                }
                // `error` com codigo interno ou de protocolo e fatal; o resto
                // (`bad_request`, `not_connected`, `send_failed`) mantem o
                // bridge vivo e nao deve derrubar o pareamento.
                if let BridgeEvent::Error { code, ref message, .. } = event
                    && matches!(code, ErrorCode::Internal | ErrorCode::Protocol)
                {
                    conn.kill().await;
                    return Err(BridgeError::Protocol(message.clone()).into());
                }

                for effect in apply(&mut machine, &event, now) {
                    match effect {
                        Effect::ShowQr { attempt } => {
                            if let Some(data) = pending_qr.take() {
                                ui.qr(
                                    &data,
                                    attempt,
                                    super::state::MAX_QR_ATTEMPTS,
                                    previous_expired,
                                );
                                previous_expired = false;
                            }
                        }
                        Effect::ArchiveSession => {
                            ui.status("a sessao anterior nao vale mais — arquivando");
                            store.archive()?;
                        }
                        Effect::PurgeSession => {
                            // Inalcancavel no `pair`: `Event::SessionDead` so
                            // e alimentado depois do codigo de saida, abaixo.
                            store.purge()?;
                            conn.kill().await;
                            return Err(RunError::SessionDead {
                                reason_code: dead_reason_code,
                            });
                        }
                        Effect::Fail(Failure::QrExpired) => {
                            conn.kill().await;
                            return Err(RunError::QrExpired);
                        }
                        Effect::Fail(_) => {
                            let hint = conn.stderr_hint();
                            conn.kill().await;
                            return Err(BridgeError::Protocol(format!(
                                "o bridge encerrou antes de concluir o pareamento{hint}"
                            ))
                            .into());
                        }
                        Effect::Ready => {
                            ui.status("sincronizando a sessao…");
                        }
                        Effect::QrExpired { .. } | Effect::ScheduleReconnect { .. } => {}
                    }
                }

                if matches!(event, BridgeEvent::Authenticated) {
                    ui.authenticated();
                }
                if machine.phase() == Phase::Connected {
                    // Nao saimos aqui: o bridge ainda vai mandar o
                    // `session_update` final antes de sair 0, e e ele que
                    // vale. Sair agora gravaria um blob sem as chaves da
                    // sincronizacao.
                    continue;
                }
            }
        }
    }

    // O filho fechou o stdout: o codigo de saida e quem decide o que aconteceu.
    let code = conn.wait().await?;
    if super::protocol::session_is_dead(code, dead_reason_code) {
        machine.on(Event::SessionDead, now);
        store.purge()?;
        return Err(RunError::SessionDead {
            reason_code: dead_reason_code,
        });
    }
    if saw_logged_out {
        // Saiu 0 depois de `logged_out`: logout pedido, ou 440
        // (`connectionReplaced` — outro aparelho assumiu). A sessao continua
        // valendo, entao NADA e apagado.
        return Err(BridgeError::Protocol(
            "outro aparelho assumiu esta sessao, ou o logout foi solicitado; \
a sessao gravada continua valendo"
                .into(),
        )
        .into());
    }
    if !saw_connected || machine.desired() == Desired::Off {
        let hint = conn.stderr_hint();
        return Err(BridgeError::Protocol(format!(
            "o bridge encerrou (codigo {code:?}) antes de conectar{hint}"
        ))
        .into());
    }

    // Persistencia: o blob PRIMEIRO. Quem grava `enabled = true` e o chamador.
    let session_saved = match latest_blob {
        Some(ref blob) if !blob.is_empty() => {
            store.save(blob, key)?;
            true
        }
        _ => false,
    };

    if !session_saved && !had_existing {
        // Conectou mas nunca mandou sessao: nao ha o que salvar, e dizer
        // "pronto" seria mentira que o gateway pagaria no proximo boot.
        return Err(BridgeError::Protocol(
            "o bridge conectou mas nao entregou a sessao — nada foi gravado".into(),
        )
        .into());
    }

    Ok(PairOutcome {
        reused_existing_session: had_existing && machine.qr_attempts() == 0,
        session_saved,
        phone_last4,
    })
}

/// Traduz um evento do bridge em evento da maquina e aplica.
fn apply(machine: &mut Machine, event: &BridgeEvent, now: u64) -> Vec<Effect> {
    match event {
        BridgeEvent::Qr {
            expires_in_secs, ..
        } => machine.on(
            Event::QrShown {
                expires_in_secs: *expires_in_secs,
            },
            now,
        ),
        BridgeEvent::Status {
            state: BridgeState::WaitingScan,
            ..
        } => machine.on(Event::ScanPending, now),
        BridgeEvent::Authenticated => machine.on(Event::Authenticated, now),
        BridgeEvent::Connected { .. } => machine.on(Event::Connected, now),
        // `disconnected{reason:"logged_out"}` e `logged_out` NAO movem a
        // maquina: eles nao provam que a sessao morreu (ver `exit_code`). Quem
        // alimenta `Event::SessionDead` e o driver, depois do codigo de saida.
        BridgeEvent::Disconnected {
            reason, will_retry, ..
        } if *reason != DisconnectReason::LoggedOut => machine.on(
            Event::Disconnected {
                will_retry: *will_retry,
            },
            now,
        ),
        // `status` de outros estados, `log`, `session_update`, `sent`, `error`
        // e eventos desconhecidos nao movem a maquina.
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Modo serve — a costura do gateway
// ---------------------------------------------------------------------------

/// Para onde as mensagens recebidas vao.
///
/// **Esta e a costura do proximo slice.** O canal pull `whatsapp_linked` do
/// gateway implementa isto e passa para [`serve`]; nada mais precisa mudar
/// neste modulo. A allowlist de remetentes e o pairing NAO sao decididos aqui:
/// o bridge entrega tudo, o sink e quem filtra — o mesmo desenho dos
/// `channel_gates` que o canal Cloud ja usa.
pub trait InboundSink: Send + Sync {
    /// Chamado por mensagem recebida. Erro aqui nao derruba a conexao: o canal
    /// decide o que fazer com a propria falha.
    fn deliver(&self, message: InboundMessage);

    /// Chamado a cada mudanca de conexao, para o `/api/diagnostics`.
    fn on_connection(&self, _jid: Option<&Jid>, _connected: bool) {}
}

/// Persistencia da sessao vista pelo modo serve.
///
/// O bridge reemite `session_update` a cada rotacao de chave; ignorar isso faz
/// a sessao envelhecer ate o servidor recusa-la.
fn persist(store: &SessionStore, key: &SessionKey, blob: &SessionBlob) {
    if let Err(e) = store.save(blob, key) {
        // Sem `?`: falhar a gravacao nao deve derrubar um canal que esta
        // funcionando. O valor NUNCA entra na mensagem — `SessionError` so
        // carrega caminho e categoria.
        tracing::warn!(error = %e, "falha ao gravar a sessao do WhatsApp vinculado");
    }
}

/// Loop de longa duracao. Reconecta com o backoff da maquina de estados ate
/// `cancel` ou ate a sessao morrer.
///
/// `outbound` e o outro lado da costura: o gateway manda
/// [`BridgeCommand::Send`], `Read` e `Typing` por ele. Os comandos mandados
/// enquanto o bridge esta caido sao **perdidos de proposito** — reenviar uma
/// resposta minutos depois de uma reconexao e pior do que nao responder, e
/// enfileirar aqui esconderia do gateway que ele precisa decidir isso.
///
/// `jitter` e injetado (o chamador passa `rand`), pelo mesmo motivo de a
/// maquina nao sortear: o teste precisa de intervalos deterministicos.
pub async fn serve(
    launcher: Arc<dyn BridgeLauncher>,
    store: SessionStore,
    key: SessionKey,
    sink: Arc<dyn InboundSink>,
    mut outbound: mpsc::Receiver<BridgeCommand>,
    mut cancel: watch::Receiver<bool>,
    jitter: impl Fn() -> f64 + Send,
) -> Result<(), RunError> {
    let mut machine = Machine::new();
    let mut attempt: u32 = 0;
    let mut now: u64 = 0;

    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        if !store.exists() {
            return Err(RunError::SessionDead { reason_code: None });
        }

        match serve_once(
            launcher.as_ref(),
            &store,
            &key,
            sink.as_ref(),
            &mut outbound,
            &mut cancel,
            &mut machine,
            &mut now,
        )
        .await
        {
            Ok(ServeExit::Cancelled) | Ok(ServeExit::Stopped) => {
                sink.on_connection(None, false);
                return Ok(());
            }
            Ok(ServeExit::SessionDead { reason_code }) => {
                store.purge()?;
                sink.on_connection(None, false);
                return Err(RunError::SessionDead { reason_code });
            }
            Ok(ServeExit::Dropped) | Err(_) => {
                sink.on_connection(None, false);
                attempt = attempt.saturating_add(1);
                let delay = backoff_ms(attempt, jitter());
                tracing::info!(
                    attempt,
                    delay_ms = delay,
                    "WhatsApp vinculado caiu; reconectando"
                );
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                    _ = cancel.changed() => return Ok(()),
                }
            }
        }
    }
}

enum ServeExit {
    Cancelled,
    /// Sessao morta (exit 2): apaga o material e para.
    SessionDead {
        reason_code: Option<i64>,
    },
    /// `logged_out` com exit 0: para, mas **nao** apaga nada.
    Stopped,
    Dropped,
}

async fn serve_once(
    launcher: &dyn BridgeLauncher,
    store: &SessionStore,
    key: &SessionKey,
    sink: &dyn InboundSink,
    outbound: &mut mpsc::Receiver<BridgeCommand>,
    cancel: &mut watch::Receiver<bool>,
    machine: &mut Machine,
    now: &mut u64,
) -> Result<ServeExit, RunError> {
    let blob = store.load(key)?;
    let mut conn = BridgeConnection::spawn(launcher).await?;
    conn.expect_started().await?;
    conn.send(&BridgeCommand::SessionLoad {
        session: Some(blob),
    })
    .await?;
    conn.send(&BridgeCommand::Start {
        mode: StartMode::Serve,
    })
    .await?;

    let mut saw_logged_out = false;
    let mut dead_reason_code: Option<i64> = None;

    loop {
        tokio::select! {
            biased;
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    let _ = conn.send(&BridgeCommand::Shutdown).await;
                    conn.kill().await;
                    return Ok(ServeExit::Cancelled);
                }
            }
            Some(command) = outbound.recv() => {
                // `session_load`, `start` e `logout` sao do driver, nao do
                // consumidor: aceita-los daqui deixaria o gateway capaz de
                // deslogar a conta por engano.
                if matches!(
                    command,
                    BridgeCommand::Send { .. }
                        | BridgeCommand::Read { .. }
                        | BridgeCommand::Typing { .. }
                ) {
                    conn.send(&command).await?;
                } else {
                    tracing::warn!("comando recusado no canal de saida do WhatsApp vinculado");
                }
            }
            event = conn.next_event() => {
                let Some(event) = event? else {
                    // stdout fechou: o codigo de saida decide se a sessao
                    // morreu ou se foi so uma queda a reconectar.
                    let code = conn.wait().await?;
                    return Ok(if super::protocol::session_is_dead(code, dead_reason_code) {
                        ServeExit::SessionDead { reason_code: dead_reason_code }
                    } else if saw_logged_out {
                        // `logged_out` sem prova de morte: logout pedido, ou
                        // 440 (outro aparelho assumiu). Para, sem apagar nada.
                        ServeExit::Stopped
                    } else {
                        ServeExit::Dropped
                    });
                };
                *now += 1;
                match event {
                    BridgeEvent::SessionUpdate { ref session, .. } => persist(store, key, session),
                    BridgeEvent::Message(ref msg) => sink.deliver((**msg).clone()),
                    BridgeEvent::Connected { ref jid, .. } => {
                        machine.on(Event::Connected, *now);
                        sink.on_connection(jid.as_ref(), true);
                    }
                    BridgeEvent::LoggedOut => saw_logged_out = true,
                    BridgeEvent::Disconnected {
                        reason: DisconnectReason::LoggedOut,
                        reason_code,
                        ..
                    } => dead_reason_code = reason_code,
                    _ => {
                        apply(machine, &event, *now);
                    }
                }
            }
        }
    }
}
