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

/// Silencio maximo do bridge antes de [`pair`] desistir, em segundos.
///
/// **O `pair` do bridge nao tem prazo proprio**: QR expirado (408) e queda de
/// rede reconectam sozinhos, com backoff ate 30 s, para sempre. Quem
/// cronometra o pareamento e este lado. O teto de 5 QRs cobre o caso normal;
/// este cobre o outro — o bridge que para de falar sem nunca mandar o proximo
/// QR (rede caida, servidor recusando o handshake). Sem ele o usuario fica com
/// um terminal parado ate lembrar do Ctrl+C.
///
/// 90 s e folgado de proposito: e mais que o maior backoff (30 s) somado a
/// validade de um QR (20 s), entao nao dispara num pareamento lento de
/// verdade. O contador zera a CADA evento, inclusive `log` e `status`.
pub const DEFAULT_STALL_AFTER_SECS: u64 = 90;

/// Silencio maximo **depois de conectar**, em segundos.
///
/// O prazo acima exclui [`Phase::Connected`] de proposito: conectado, o
/// `pair` para de contar e espera o `session_update` final, porque sair antes
/// gravaria um blob sem as chaves da sincronizacao. So que "espera" sem prazo
/// e pendurar o terminal para sempre — uma ponte que conecta e nunca fecha o
/// stdout nao tem nenhum outro prazo, nem do lado dela nem daqui.
///
/// Este e o teto desse ultimo trecho. Estourá-lo **nao** e erro: o que ja
/// chegou e gravado pelo caminho normal de saida, e a ausencia do blob vira a
/// mesma mensagem explicita de sempre. 60 s e muito mais do que a
/// sincronizacao final leva e muito menos do que "para sempre".
pub const DEFAULT_FINAL_FLUSH_SECS: u64 = 60;

/// Teto do "a ponte fala e nunca progride", em segundos.
///
/// # Por que o watchdog de silencio nao cobre isto
///
/// [`DEFAULT_STALL_AFTER_SECS`] mede **silencio**, e o contador dele zera a
/// cada evento. Existe um fracasso que nao e silencioso: o bridge que
/// reconecta sem parar e emite `disconnected{will_retry:true}` + `status` a
/// cada rodada de backoff — no maximo a cada 30 s, sempre abaixo dos 90 s do
/// prazo de silencio. E o usuario atras de captive portal, com a porta 443
/// bloqueada ou com o relogio do sistema errado: a ponte fala, o watchdog
/// existe, funciona, e **nunca dispara**, porque o proprio fracasso realimenta
/// o relogio dele.
///
/// Este prazo e o outro: ele mede **tempo sem progresso** e **nao zera com
/// evento nenhum**. Quem o renova e a FASE do pareamento — ha QR na tela, ou
/// o servidor aceitou a sessao (ver [`is_progress`]) —, e nao a chegada de um
/// evento. Enquanto o pareamento nao andar, os dois relogios correm juntos e
/// este e o que cobre o caso em que o outro e realimentado.
///
/// # Por que um relogio, e nao "ja progrediu alguma vez"
///
/// A primeira versao deste teto era um booleano pegajoso: o primeiro QR o
/// ligava e nada o desligava. Isso deixava um terceiro caminho de travamento
/// aberto — a rede caindo logo DEPOIS de o QR aparecer. Ali os tres tetos do
/// driver ficam desarmados ao mesmo tempo: o de silencio porque cada
/// `disconnected` o realimenta, o de QR porque a maquina estaciona em
/// `Phase::QrRequired` (onde [`super::state::Machine::tick`] nao tem mais
/// nada a expirar, ja que o contador de tentativas so anda com um evento `qr`
/// novo) e este, pelo booleano. Com os prazos de producao o comando nao
/// terminava nunca. O cenario `qr-then-retry-forever` da fixture e ele.
///
/// 120 s e folgado: quatro backoffs no teto da ponte cabem dentro. E um
/// usuario lento para pegar o celular nao e cortado, porque o QR na tela
/// renova o relogio a cada tick — quem limita esse caso e o teto de
/// [`super::state::MAX_QR_ATTEMPTS`] QRs, que e o teto certo para ele.
pub const DEFAULT_NO_PROGRESS_AFTER_SECS: u64 = 120;

/// Ajustes de [`pair`]. Existe para o teste poder encurtar os prazos de
/// silencio sem esperar minutos de relogio real; producao usa o [`Default`].
#[derive(Debug, Clone, Copy)]
pub struct PairOptions {
    /// Silencio maximo ANTES de conectar.
    pub stall_after_secs: u64,
    /// Silencio maximo DEPOIS de conectar.
    pub final_flush_secs: u64,
    /// Teto do "falou o tempo todo e nunca progrediu". **Nao zera com evento.**
    pub no_progress_after_secs: u64,
}

impl Default for PairOptions {
    fn default() -> Self {
        Self {
            stall_after_secs: DEFAULT_STALL_AFTER_SECS,
            final_flush_secs: DEFAULT_FINAL_FLUSH_SECS,
            no_progress_after_secs: DEFAULT_NO_PROGRESS_AFTER_SECS,
        }
    }
}

/// Ajustes de [`serve`]. Mesmo papel do [`PairOptions`], e pelo mesmo motivo:
/// sem ele os prazos do `serve` eram constantes de 90 s, e um teste que os
/// exercitasse precisaria de um minuto e meio de relogio real — ou seja,
/// nenhum teste os exercitava.
#[derive(Debug, Clone, Copy)]
pub struct ServeOptions {
    /// Silencio maximo **antes de conectar**, em segundos. Vale para o
    /// handshake, para o `send` do driver e para o laco de eventos.
    ///
    /// # Por que so antes de conectar
    ///
    /// Depois do `connected` o silencio e o estado NORMAL: uma conta sem
    /// mensagem nenhuma fica quieta por horas, e cortar por isso derrubaria o
    /// canal saudavel a cada madrugada. Antes do `connected`, silencio e
    /// travamento — e e o caso que nao tinha relogio nenhum.
    pub stall_after_secs: u64,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            stall_after_secs: DEFAULT_STALL_AFTER_SECS,
        }
    }
}

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
    cancel: watch::Receiver<bool>,
) -> Result<PairOutcome, RunError> {
    pair_with(launcher, store, key, ui, cancel, PairOptions::default()).await
}

/// [`pair`] com os prazos explicitos.
pub async fn pair_with(
    launcher: &dyn BridgeLauncher,
    store: &SessionStore,
    key: &SessionKey,
    ui: &mut dyn PairUi,
    mut cancel: watch::Receiver<bool>,
    options: PairOptions,
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
    //
    // Esta linha de status sai ANTES do handshake de proposito: ela e o
    // primeiro sinal de vida que o usuario recebe depois das instrucoes, e sem
    // ela um bridge que demora deixa a tela parada no texto do QR sem nada
    // dizendo que alguma coisa esta acontecendo.
    ui.status("iniciando o bridge…");
    let mut conn = BridgeConnection::spawn(launcher).await?;
    // Daqui ate o `tokio::select!` do laco, cada etapa roda sob prazo e com o
    // Ctrl+C valendo — ver [`step_with_deadline`].
    let handshake =
        step_with_deadline(&mut cancel, options.stall_after_secs, conn.expect_started()).await;
    let started = match handshake {
        Ok(Step::Done(ev)) => ev,
        Ok(Step::Cancelled) => {
            conn.kill().await;
            return Err(RunError::Cancelled);
        }
        Ok(Step::TimedOut) => {
            let hint = conn.stderr_hint();
            conn.kill().await;
            return Err(handshake_timeout("o handshake", options.stall_after_secs, &hint).into());
        }
        Err(e) => {
            conn.kill().await;
            return Err(e.into());
        }
    };
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

    for (what, command) in [
        (
            "`session_load`",
            BridgeCommand::SessionLoad { session: existing },
        ),
        (
            "`start`",
            BridgeCommand::Start {
                mode: StartMode::Pair,
            },
        ),
    ] {
        let sent =
            step_with_deadline(&mut cancel, options.stall_after_secs, conn.send(&command)).await;
        match sent {
            Ok(Step::Done(())) => {}
            Ok(Step::Cancelled) => {
                conn.kill().await;
                return Err(RunError::Cancelled);
            }
            Ok(Step::TimedOut) => {
                let hint = conn.stderr_hint();
                conn.kill().await;
                return Err(handshake_timeout(what, options.stall_after_secs, &hint).into());
            }
            Err(e) => {
                conn.kill().await;
                return Err(e.into());
            }
        }
    }
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
    // Segundo do ultimo evento vindo do bridge. Qualquer evento conta como
    // sinal de vida, inclusive `log`.
    let mut last_event_secs: u64 = 0;
    // Segundo em que o pareamento esteve pela ultima vez numa fase que
    // ANDA. E um relogio, e nao um booleano: "ja progrediu uma vez" deixava
    // um unico QR desarmar o teto para sempre, e entao a rede caindo logo
    // depois do QR rodava sem fim. Quem o renova e a FASE (ver
    // [`is_progress`]), nao o evento — evento de fracasso e exatamente o que
    // realimenta o outro relogio, o de silencio.
    let mut last_progress_secs: u64 = 0;

    loop {
        tokio::select! {
            // Cancelamento vence: Ctrl+C nao pode ficar atras de um evento.
            biased;

            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    machine.on(Event::Stop, now);
                    // Pede saida limpa: `shutdown` fecha o socket SEM deslogar,
                    // entao uma sessao ja valida sobrevive ao cancelamento.
                    shutdown_politely(&mut conn).await;
                    conn.kill().await;
                    return Err(RunError::Cancelled);
                }
            }

            _ = ticker.tick() => {
                now += 1;
                let silent_for = now.saturating_sub(last_event_secs);
                // Antes do teste, e medido pela fase: enquanto houver QR na
                // tela o relogio anda junto com o `now` e o prazo nunca
                // vence. `QrRequired` (o QR expirou e o proximo nao veio) e
                // `Reconnecting` NAO renovam — sao justamente as fases em que
                // o pareamento parou.
                if is_progress(machine.phase()) {
                    last_progress_secs = now;
                }
                if now.saturating_sub(last_progress_secs) >= options.no_progress_after_secs {
                    // A ponte falou o tempo todo e o pareamento nao andou:
                    // nem um QR na tela, nem uma conexao. O prazo de silencio
                    // acima nunca dispararia aqui — cada tentativa fracassada
                    // o zera. Ver [`DEFAULT_NO_PROGRESS_AFTER_SECS`].
                    let hint = conn.stderr_hint();
                    shutdown_politely(&mut conn).await;
                    conn.kill().await;
                    return Err(BridgeError::Protocol(format!(
                        "o bridge tentou por {}s sem chegar a um QR nem conectar. \
        Quase sempre e a rede: portal de autenticacao (wi-fi de hotel/aeroporto) ainda \
        nao aceito, saida para a porta 443 bloqueada, ou o relogio do sistema errado. \
        Confira a conexao e rode `garra whatsapp` de novo.{hint}",
                        options.no_progress_after_secs
                    ))
                    .into());
                }
                if machine.phase() == Phase::Connected {
                    if silent_for >= options.final_flush_secs {
                        // Conectado e mudo: o `session_update` final ou ja
                        // chegou, ou nao vem mais. Fecha pelo caminho normal
                        // de saida — o que chegou e gravado, e a falta dele
                        // vira erro explicito la embaixo.
                        shutdown_politely(&mut conn).await;
                        conn.kill().await;
                        break;
                    }
                } else if silent_for >= options.stall_after_secs {
                    let hint = conn.stderr_hint();
                    shutdown_politely(&mut conn).await;
                    conn.kill().await;
                    return Err(BridgeError::Protocol(format!(
                        "o bridge parou de responder por {}s sem concluir o pareamento{hint}",
                        options.stall_after_secs
                    ))
                    .into());
                }
                for effect in machine.tick(now) {
                    match effect {
                        Effect::QrExpired { .. } => previous_expired = true,
                        Effect::Fail(Failure::QrExpired) => {
                            shutdown_politely(&mut conn).await;
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
                last_event_secs = now;

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
                // Uma queda que vai ser retentada precisa APARECER. Sem esta
                // linha o unico caminho de fracasso que a ponte sabe percorrer
                // sozinha — reconectar para sempre — nao imprime nada, e a
                // tela fica parada em "conectando ao WhatsApp…" ate o prazo
                // acima. Uma tela que se move e explica ja nao e o estado que
                // o dono proibiu.
                if let BridgeEvent::Disconnected {
                    ref reason,
                    will_retry: true,
                    retry_in_ms,
                    ..
                } = event
                    && *reason != DisconnectReason::LoggedOut
                {
                    ui.status(&retry_line(*reason, retry_in_ms));
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

                let effects = apply(&mut machine, &event, now);
                // Depois de aplicar, e nao antes: e a maquina que sabe se o
                // evento moveu o pareamento para frente. Aqui e no braco do
                // ticker pela mesma regra — o relogio segue a FASE — para que
                // um QR de vida curta, que nasca e expire entre dois ticks,
                // ainda conte como progresso.
                if is_progress(machine.phase()) {
                    last_progress_secs = now;
                }
                for effect in effects {
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
                            // `discard_dead_session` e nao `purge` pelo mesmo
                            // motivo do desfecho la embaixo.
                            store.discard_dead_session()?;
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
    //
    // Sob prazo pelo mesmo motivo do handshake: um filho que fecha o stdout e
    // nao termina prende este `wait` para sempre, e aqui ja nao ha nenhum
    // outro relogio — o laco acabou.
    let waited = step_with_deadline(&mut cancel, options.stall_after_secs, conn.wait()).await;
    let code = match waited {
        Ok(Step::Done(code)) => code,
        Ok(Step::Cancelled) => {
            conn.kill().await;
            return Err(RunError::Cancelled);
        }
        Ok(Step::TimedOut) => {
            let hint = conn.stderr_hint();
            conn.kill().await;
            return Err(BridgeError::Protocol(format!(
                "o bridge fechou a saida mas nao terminou em {}s — foi encerrado a forca. \
Rode `garra whatsapp` de novo.{hint}",
                options.stall_after_secs
            ))
            .into());
        }
        Err(e) => {
            conn.kill().await;
            return Err(e.into());
        }
    };
    if super::protocol::session_is_dead(code, dead_reason_code) {
        machine.on(Event::SessionDead, now);
        // NAO e `purge`: num re-vinculo o `session.enc.prev` e a sessao boa
        // que o `link` acabou de arquivar, e `purge` a triturava junto com a
        // chave — o guard do chamador ficava sem nada para restaurar. O que
        // este pareamento pode descartar e o blob que o servidor recusou.
        // Ver [`SessionStore::discard_dead_session`].
        store.discard_dead_session()?;
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
            // O blob novo esta em disco: a sessao arquivada cumpriu seu papel
            // e nao pode ficar para tras como material de autenticacao vivo
            // num arquivo esquecido (decisao 3 do ADR 0023).
            store.discard_archive()?;
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

/// Desfecho de uma etapa do handshake rodada sob prazo.
///
/// Existe porque as tres respostas — pronto, o usuario desistiu, o prazo
/// estourou — precisam de tratamentos diferentes no chamador, e so ele tem a
/// mensagem certa para cada uma. Um `Result<T, BridgeError>` achataria
/// "cancelado" em "erro".
enum Step<T> {
    Done(T),
    /// Ctrl+C durante a etapa.
    Cancelled,
    /// O prazo estourou sem resposta.
    TimedOut,
}

/// Roda uma etapa do handshake com prazo **e** com o Ctrl+C valendo.
///
/// # Por que isto existe
///
/// O `tokio::select!` do laco principal — com o relogio, o watchdog de
/// silencio e o braco de cancelamento — so comeca DEPOIS do handshake. Tudo o
/// que vem antes dele (`expect_started`, os dois `send`) rodava sem prazo e
/// sem cancelamento: um `node` que sobe e nao fala (shim de asdf/volta/nvm
/// baixando versao, stub de snap esperando confirmacao, wrapper que le stdin)
/// pendurava o terminal para sempre, e nem o primeiro nem o segundo Ctrl+C
/// faziam nada — a CLI ja tinha trocado o SIGINT default por um canal que
/// ninguem estava lendo.
///
/// O `send` esta aqui pelo mesmo motivo, e nao por simetria: `session_load`
/// carrega o blob inteiro, e acima do buffer do pipe (64 KiB no Linux) um
/// filho que nao le trava o `write_all`.
///
/// `biased` para que o cancelamento nunca fique atras do prazo, e o laco para
/// que um `changed()` com valor falso — um `send(false)` de quem quer que seja
/// — nao vire cancelamento, exatamente como no laco principal.
async fn step_with_deadline<T, F>(
    cancel: &mut watch::Receiver<bool>,
    secs: u64,
    fut: F,
) -> Result<Step<T>, BridgeError>
where
    F: std::future::Future<Output = Result<T, BridgeError>>,
{
    let deadline = tokio::time::timeout(Duration::from_secs(secs), fut);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            biased;

            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Ok(Step::Cancelled);
                }
            }

            done = &mut deadline => {
                return match done {
                    Ok(Ok(value)) => Ok(Step::Done(value)),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Ok(Step::TimedOut),
                };
            }
        }
    }
}

/// Prazo maximo do `shutdown` de cortesia. Ver [`shutdown_politely`].
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// Pede saida limpa ao bridge **sob prazo**, e desiste sem drama.
///
/// `shutdown` fecha o socket SEM deslogar, entao uma sessao ja valida
/// sobrevive — vale a pena tentar. O que nao vale e esperar para sempre:
/// `conn.send` e `write_all` + `flush` no stdin do filho, e um filho que
/// parou de ler trava a escrita assim que o buffer do pipe (64 KiB) enche. O
/// irmao deste caminho, no handshake, ja rodava sob prazo exatamente por
/// isso.
///
/// E aqui e pior do que la, porque **este e o ultimo recurso**: todos os
/// `send(&Shutdown)` deste modulo acontecem num caminho de desistencia, o do
/// cancelamento inclusive. Um `Ctrl+C` que fica preso na cortesia e o mesmo
/// terminal parado que o resto desta rodada existe para fechar.
///
/// O `conn.kill()` vem logo depois em todos os chamadores, entao falhar aqui
/// nao deixa processo para tras.
async fn shutdown_politely(conn: &mut BridgeConnection) {
    let _ = tokio::time::timeout(SHUTDOWN_GRACE, conn.send(&BridgeCommand::Shutdown)).await;
}

/// A mensagem de um prazo estourado no handshake.
///
/// Destravar nao basta: quem le isto precisa saber o que aconteceu e o que
/// fazer. O culpado quase sempre e o `node` da PATH, e o comando que confirma
/// isso cabe na mesma linha.
fn handshake_timeout(what: &str, secs: u64, hint: &str) -> BridgeError {
    BridgeError::Protocol(format!(
        "o bridge nao respondeu {what} em {secs}s. \
O `node` da PATH pode estar preso antes de rodar o bridge — um shim (asdf, \
volta, nvm, corepack) baixando versao, ou um stub esperando confirmacao. \
Rode `node --version` a mao para ver se ele responde, e depois \
`garra whatsapp` de novo.{hint}"
    ))
}

/// O pareamento esta ANDANDO? E o que renova o relogio de
/// [`DEFAULT_NO_PROGRESS_AFTER_SECS`].
///
/// "Progredir" e uma coisa so: ou ha um QR na tela para o usuario ler, ou o
/// servidor aceitou a sessao. Tudo o mais — `status`, `log`, `disconnected`
/// com retry — e ruido que a ponte produz enquanto nao chega a lugar nenhum,
/// e e exatamente esse ruido que realimenta o outro relogio.
///
/// Repare no que **nao** esta na lista, e por que: [`Phase::QrRequired`] e a
/// fase em que o QR anterior expirou e o proximo ainda nao veio. Ela e a fase
/// de um pareamento PARADO, ainda que a ponte esteja falando sem parar — e e
/// nela que o cenario `qr-then-retry-forever` estaciona para sempre.
/// [`Phase::Reconnecting`] tem a mesma forma. Incluir qualquer uma das duas
/// aqui desarmaria o teto do mesmo jeito que o booleano pegajoso desarmava.
fn is_progress(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::QrGenerated { .. }
            | Phase::WaitingScan { .. }
            | Phase::Authenticated
            | Phase::Connected
    )
}

/// A linha que o usuario le quando a ponte cai e vai tentar de novo.
///
/// Sem motivo e sem prazo isto seria so mais uma linha rolando: o que a torna
/// util e dizer **o que** falhou e **quando** e a proxima tentativa, porque e
/// o que permite ao usuario decidir se espera ou se conserta a rede.
fn retry_line(reason: DisconnectReason, retry_in_ms: Option<u64>) -> String {
    let motivo = match reason {
        DisconnectReason::Network => "a rede caiu",
        DisconnectReason::Timeout => "o servidor nao respondeu a tempo",
        DisconnectReason::RestartRequired => "o WhatsApp pediu para reiniciar a conexao",
        DisconnectReason::Replaced => "outro aparelho assumiu a conexao",
        // `logged_out` nao chega aqui (o chamador filtra) e `unknown` e o
        // resto: nao invente um motivo que nao se sabe.
        DisconnectReason::LoggedOut | DisconnectReason::Unknown => "a conexao caiu",
    };
    match retry_in_ms {
        // Arredonda para CIMA, e nao trunca. Com `ms / 1000` um backoff de
        // 1500 ms aparecia como "1s", e o usuario via a linha parada meio
        // segundo depois do prazo que ela mesma prometeu — pequeno, mas e
        // exatamente o tipo de desencontro que faz alguem achar que a tela
        // travou. Para cima, e nao ao mais proximo, porque errar cedo
        // (prometer 2s e tentar em 1,5s) o usuario nem nota, e errar tarde e
        // a linha vencida de novo. O backoff desta ponte escala em 1,5x,
        // entao 1500 ms e um degrau que acontece de verdade.
        Some(ms) if ms >= 500 => format!("{motivo} — nova tentativa em {}s", ms.div_ceil(1000)),
        Some(_) | None => format!("{motivo} — tentando de novo"),
    }
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

/// Loop de longa duracao. Reconecta com backoff proprio ate `cancel` ou ate a
/// sessao morrer.
///
/// # O `serve` NAO usa a [`Machine`], e isso e deliberado
///
/// A maquina de estados e do [`pair`]. Aqui ela era alimentada e nunca lida —
/// dava para arrancar as duas chamadas e nenhum teste piscava. Em vez de
/// fingir, o acoplamento saiu; as tres razoes pelas quais liga-la de verdade
/// seria pior:
///
/// 1. **A queda que mais acontece nao emite evento.** O filho morre (crash,
///    OOM, `kill`) e o stdout so fecha. O unico evento que a maquina recebe
///    disso e [`Event::BridgeExited`], que a partir de `Connected` e terminal
///    ([`Failure::BridgeGone`]) — de proposito, porque e assim que o `pair`
///    para. Dirigir o `serve` pela maquina significaria nunca reconectar
///    depois de um crash, ou inverter uma transicao da qual o `pair` depende.
/// 2. **[`Effect::ScheduleReconnect`] vem com jitter zero.** A maquina e pura
///    e nao sorteia: ela chama `backoff_ms(attempt, 0.0)`. Consumir o efeito
///    jogaria fora o jitter que este loop injeta — o mesmo jitter que o doc do
///    `state.rs` diz existir para N instancias nao reconectarem no mesmo
///    milissegundo.
/// 3. **Um `Machine` resetado a cada tentativa nao guarda nada** que o
///    contador `attempt` daqui ja nao guarde.
///
/// A metade de reconexao do `state.rs` continua publica e testada na tabela
/// unitaria, para quem quiser um driver dirigido por ela; nenhum driver deste
/// modulo e.
///
/// `outbound` e o outro lado da costura: o gateway manda
/// [`BridgeCommand::Send`], `Read` e `Typing` por ele. Os comandos mandados
/// enquanto o bridge esta caido sao **perdidos de proposito** — reenviar uma
/// resposta minutos depois de uma reconexao e pior do que nao responder, e
/// enfileirar aqui esconderia do gateway que ele precisa decidir isso.
///
/// `jitter` e injetado (o chamador passa `rand`), pelo mesmo motivo de a
/// maquina nao sortear: o teste precisa de intervalos deterministicos — e
/// **este backoff e o unico que existe no `serve`**. O teste nao conta as
/// chamadas de `jitter`, ele **cronometra** o intervalo entre duas: contar
/// prova que o atraso foi calculado, nao que alguem esperou, e arrancar o
/// `sleep` deixava a contagem intacta.
///
/// O contador de tentativas volta a zero a cada execucao que chegou a
/// conectar: ele mede quedas SEGUIDAS, nao quedas acumuladas na vida do
/// processo.
pub async fn serve(
    launcher: Arc<dyn BridgeLauncher>,
    store: SessionStore,
    key: SessionKey,
    sink: Arc<dyn InboundSink>,
    outbound: mpsc::Receiver<BridgeCommand>,
    cancel: watch::Receiver<bool>,
    jitter: impl Fn() -> f64 + Send,
) -> Result<(), RunError> {
    serve_with(
        launcher,
        store,
        key,
        sink,
        outbound,
        cancel,
        jitter,
        ServeOptions::default(),
    )
    .await
}

/// [`serve`] com os prazos injetados. Producao usa o [`Default`]; o teste
/// encurta-os para exercitar, em segundos, o que de outro modo levaria 90.
#[allow(clippy::too_many_arguments)]
pub async fn serve_with(
    launcher: Arc<dyn BridgeLauncher>,
    store: SessionStore,
    key: SessionKey,
    sink: Arc<dyn InboundSink>,
    mut outbound: mpsc::Receiver<BridgeCommand>,
    mut cancel: watch::Receiver<bool>,
    jitter: impl Fn() -> f64 + Send,
    options: ServeOptions,
) -> Result<(), RunError> {
    let mut attempt: u32 = 0;

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
            options,
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
            outcome @ (Ok(ServeExit::Dropped { .. }) | Err(_)) => {
                sink.on_connection(None, false);
                attempt = next_attempt(
                    attempt,
                    matches!(
                        outcome,
                        Ok(ServeExit::Dropped {
                            was_connected: true
                        })
                    ),
                );
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

/// Proximo valor do contador de tentativas do [`serve`].
///
/// Uma execucao que chegou a conectar volta o contador ao primeiro degrau.
/// Sem isso o `attempt` so cresce: um gateway de longa duracao que caia uma
/// vez por dia caminha ate o teto de 30 s e fica la, e a queda seguinte — a
/// que acontece depois de meses no ar — espera meio minuto por nada. O
/// contador mede **quedas seguidas sem sucesso**, que e o que o backoff
/// exponencial existe para punir.
///
/// Funcao nomeada, e nao duas linhas dentro do `loop`, porque assim a regra e
/// testavel sem relogio, sem processo filho e sem esperar dois backoffs reais
/// — o mesmo motivo de a maquina de estados nao ter relogio.
fn next_attempt(current: u32, was_connected: bool) -> u32 {
    if was_connected {
        1
    } else {
        current.saturating_add(1)
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
    /// Caiu e vale reconectar. `was_connected` diz se esta execucao chegou a
    /// receber um `connected` — e o que separa "a ponte estava no ar e caiu"
    /// de "a ponte nem subiu", que e a diferenca entre zerar o backoff e
    /// deixa-lo crescer.
    Dropped {
        was_connected: bool,
    },
}

#[allow(clippy::too_many_arguments)]
async fn serve_once(
    launcher: &dyn BridgeLauncher,
    store: &SessionStore,
    key: &SessionKey,
    sink: &dyn InboundSink,
    outbound: &mut mpsc::Receiver<BridgeCommand>,
    cancel: &mut watch::Receiver<bool>,
    options: ServeOptions,
) -> Result<ServeExit, RunError> {
    let blob = store.load(key)?;
    let mut conn = BridgeConnection::spawn(launcher).await?;
    // O mesmo prazo do `pair`, pela mesma razao, com uma diferenca de
    // consequencia: aqui ninguem esta olhando o terminal. Sem ele um `node`
    // preso no handshake nao pendura uma tela — pendura o boot do gateway, e a
    // reconexao com backoff que existe logo acima nunca chega a rodar.
    let handshake = step_with_deadline(
        &mut *cancel,
        options.stall_after_secs,
        conn.expect_started(),
    )
    .await;
    match handshake {
        Ok(Step::Done(_)) => {}
        Ok(Step::Cancelled) => {
            conn.kill().await;
            return Ok(ServeExit::Cancelled);
        }
        Ok(Step::TimedOut) => {
            let hint = conn.stderr_hint();
            conn.kill().await;
            return Err(handshake_timeout("o handshake", options.stall_after_secs, &hint).into());
        }
        Err(e) => {
            conn.kill().await;
            return Err(e.into());
        }
    }

    for (what, command) in [
        (
            "`session_load`",
            BridgeCommand::SessionLoad {
                session: Some(blob),
            },
        ),
        (
            "`start`",
            BridgeCommand::Start {
                mode: StartMode::Serve,
            },
        ),
    ] {
        let sent =
            step_with_deadline(&mut *cancel, options.stall_after_secs, conn.send(&command)).await;
        match sent {
            Ok(Step::Done(())) => {}
            Ok(Step::Cancelled) => {
                conn.kill().await;
                return Ok(ServeExit::Cancelled);
            }
            Ok(Step::TimedOut) => {
                let hint = conn.stderr_hint();
                conn.kill().await;
                return Err(handshake_timeout(what, options.stall_after_secs, &hint).into());
            }
            Err(e) => {
                conn.kill().await;
                return Err(e.into());
            }
        }
    }

    let mut saw_logged_out = false;
    let mut dead_reason_code: Option<i64> = None;
    let mut saw_connected = false;

    // O relogio que faltava. `conn.send` e `conn.wait` ganharam prazo antes;
    // `conn.next_event()` nao tinha nenhum, e uma ponte que diz `started`,
    // aceita o `start` e emudece **sem fechar o stdout** prendia `serve_once`
    // para sempre — com o backoff logo acima nunca chegando a rodar. O canal
    // ficava morto em silencio, sem ninguem olhando um terminal.
    let mut now: u64 = 0;
    let mut last_event_secs: u64 = 0;
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await; // o primeiro tick e imediato

    loop {
        tokio::select! {
            biased;
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    shutdown_politely(&mut conn).await;
                    conn.kill().await;
                    return Ok(ServeExit::Cancelled);
                }
            }
            // `if !saw_connected` NAO e otimizacao: depois do `connected` o
            // silencio e o estado normal — uma conta sem mensagem nenhuma
            // fica quieta por horas —, e um prazo aqui derrubaria o canal
            // saudavel toda madrugada. Antes do `connected`, silencio e
            // travamento.
            _ = ticker.tick(), if !saw_connected => {
                now += 1;
                if now.saturating_sub(last_event_secs) >= options.stall_after_secs {
                    tracing::warn!(
                        secs = options.stall_after_secs,
                        "o bridge subiu e emudeceu sem conectar; reconectando"
                    );
                    shutdown_politely(&mut conn).await;
                    conn.kill().await;
                    return Ok(ServeExit::Dropped { was_connected: false });
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
                    // Sob prazo pelo mesmo motivo do handshake: um filho que
                    // parou de ler o stdin trava o `write_all` acima do buffer
                    // do pipe, e aqui nao ha ninguem olhando o terminal. Cair
                    // no backoff e melhor do que pendurar o canal.
                    match step_with_deadline(
                        &mut *cancel,
                        options.stall_after_secs,
                        conn.send(&command),
                    )
                    .await
                    {
                        Ok(Step::Done(())) => {}
                        Ok(Step::Cancelled) => {
                            conn.kill().await;
                            return Ok(ServeExit::Cancelled);
                        }
                        Ok(Step::TimedOut) => {
                            tracing::warn!(
                                secs = options.stall_after_secs,
                                "o bridge nao aceitou o comando no prazo; reconectando"
                            );
                            conn.kill().await;
                            return Ok(ServeExit::Dropped {
                                was_connected: saw_connected,
                            });
                        }
                        Err(e) => {
                            conn.kill().await;
                            return Err(e.into());
                        }
                    }
                } else {
                    tracing::warn!("comando recusado no canal de saida do WhatsApp vinculado");
                }
            }
            event = conn.next_event() => {
                last_event_secs = now;
                let Some(event) = event? else {
                    // stdout fechou: o codigo de saida decide se a sessao
                    // morreu ou se foi so uma queda a reconectar.
                    //
                    // Sob prazo pelo mesmo motivo do gemeo no `pair`: um filho
                    // que fecha o stdout e nao termina prende este `wait` para
                    // sempre, e aqui o laco ja acabou — nao ha nenhum outro
                    // relogio, nem Ctrl+C, nem o backoff que esta logo acima.
                    let code = match step_with_deadline(
                        &mut *cancel,
                        options.stall_after_secs,
                        conn.wait(),
                    )
                    .await
                    {
                        Ok(Step::Done(code)) => code,
                        Ok(Step::Cancelled) => {
                            conn.kill().await;
                            return Ok(ServeExit::Cancelled);
                        }
                        Ok(Step::TimedOut) => {
                            // Sem codigo de saida nao da para afirmar que a
                            // sessao morreu, e apagar material por falta de
                            // prova seria o erro pior dos dois: trata como
                            // queda e deixa o backoff decidir.
                            tracing::warn!(
                                secs = options.stall_after_secs,
                                "o bridge fechou a saida e nao terminou; encerrando a forca"
                            );
                            conn.kill().await;
                            return Ok(ServeExit::Dropped {
                                was_connected: saw_connected,
                            });
                        }
                        Err(e) => {
                            conn.kill().await;
                            return Err(e.into());
                        }
                    };
                    return Ok(if super::protocol::session_is_dead(code, dead_reason_code) {
                        ServeExit::SessionDead { reason_code: dead_reason_code }
                    } else if saw_logged_out {
                        // `logged_out` sem prova de morte: logout pedido, ou
                        // 440 (outro aparelho assumiu). Para, sem apagar nada.
                        ServeExit::Stopped
                    } else {
                        ServeExit::Dropped { was_connected: saw_connected }
                    });
                };
                match event {
                    BridgeEvent::SessionUpdate { ref session, .. } => persist(store, key, session),
                    BridgeEvent::Message(ref msg) => sink.deliver((**msg).clone()),
                    BridgeEvent::Connected { ref jid, .. } => {
                        saw_connected = true;
                        sink.on_connection(jid.as_ref(), true);
                    }
                    BridgeEvent::LoggedOut => saw_logged_out = true,
                    BridgeEvent::Disconnected {
                        reason: DisconnectReason::LoggedOut,
                        reason_code,
                        ..
                    } => dead_reason_code = reason_code,
                    // Os demais eventos nao movem nada aqui: quem decide
                    // reconexao neste driver e o codigo de saida do filho, no
                    // braco acima, e nao uma fase de maquina.
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A linha que o usuario le quando a ponte cai — e o prazo que ela
    /// promete.
    ///
    /// `ms / 1000` truncava: 1500 ms viravam "1s", e meio segundo depois a
    /// linha ja estava vencida na tela. Num comando cujo defeito recorrente e
    /// "a tela nao anda", uma linha que promete errado e do mesmo genero.
    #[test]
    fn the_retry_line_never_promises_a_deadline_that_already_passed() {
        for (ms, esperado) in [(1000, "1s"), (1500, "2s"), (2000, "2s"), (2500, "3s")] {
            let linha = retry_line(DisconnectReason::Network, Some(ms));
            assert!(
                linha.ends_with(&format!("em {esperado}")),
                "{ms} ms deviam virar `{esperado}`, e viraram: {linha}"
            );
        }
        // Abaixo de meio segundo nao ha prazo que valha a pena imprimir.
        for ms in [None, Some(0), Some(200)] {
            let linha = retry_line(DisconnectReason::Network, ms);
            assert!(
                linha.ends_with("tentando de novo"),
                "sem prazo util a linha nao pode inventar um: {linha}"
            );
        }
        // E o motivo continua sendo o que a ponte reportou.
        assert!(
            retry_line(DisconnectReason::Timeout, Some(1000)).starts_with("o servidor"),
            "o motivo nao pode ser trocado pelo generico"
        );
    }

    /// O contador de tentativas do `serve`, e o que ele significa para o
    /// atraso real. A segunda asserção e a que importa: zerar o contador so
    /// vale se o ATRASO voltar ao primeiro degrau.
    #[test]
    fn a_connection_that_worked_puts_the_backoff_back_on_the_first_step() {
        // Sem conectar, o contador sobe e o atraso sobe com ele.
        assert_eq!(next_attempt(0, false), 1);
        assert_eq!(next_attempt(1, false), 2);
        assert_eq!(next_attempt(9, false), 10);
        assert!(backoff_ms(next_attempt(9, false), 0.0) > backoff_ms(1, 0.0));

        // Tendo conectado, volta ao primeiro degrau — de qualquer altura.
        assert_eq!(next_attempt(1, true), 1);
        assert_eq!(next_attempt(9, true), 1);
        assert_eq!(next_attempt(u32::MAX, true), 1);
        assert_eq!(
            backoff_ms(next_attempt(9, true), 0.0),
            backoff_ms(1, 0.0),
            "depois de uma conexao que valeu, a proxima espera e a menor de todas"
        );

        // E o contador nunca estoura: `serve` roda por meses.
        assert_eq!(next_attempt(u32::MAX, false), u32::MAX);
    }
}
