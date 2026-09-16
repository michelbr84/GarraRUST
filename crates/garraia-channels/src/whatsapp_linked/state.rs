//! Maquina de estados do vinculo, **pura**.
//!
//! # Forma
//!
//! Mesmo contrato do `state.rs` do `garraia-desktop-core` e do `spinner.rs` da
//! CLI: nada aqui le relogio, dorme, abre socket ou toca disco. O estado so
//! muda quando alguem chama [`Machine::on`] (fato vindo do bridge) ou
//! [`Machine::tick`] (passagem de tempo, com o "agora" **injetado**). O jitter
//! do backoff tambem e injetado.
//!
//! E isso que permite testar expiracao de QR, teto de tentativas e escalada de
//! backoff numa tabela, sem processo e sem `sleep`.
//!
//! # Duas verdades separadas
//!
//! [`Desired`] e o que o usuario pediu; [`Phase`] e onde o vinculo esta. Sao
//! separadas pelo mesmo motivo do desktop-core: uma queda de rede nao pode ser
//! confundida com "o usuario desligou". A invariante que o teste fixa e a
//! inversa e mais importante — **`Desired::Off` nunca chega a
//! [`Phase::Connected`]**: depois de um `Ctrl+C` nenhum evento atrasado do
//! bridge pode ressuscitar a conexao e, com ela, gravar uma sessao que o
//! usuario cancelou.
//!
//! # Transicoes
//!
//! ```text
//!                    NoSession
//!   NotConnected ───────────────> QrRequired ──QrShown──> QrGenerated ──ScanPending──> WaitingScan
//!        │                             ^                       │ (tick: expirou)
//!        │ SessionFound                └───────────────────────┘  ate MAX_QR_ATTEMPTS
//!        v                                                       └─ excedeu ─> Failed(QrExpired)
//!   SessionFound ──BridgeStarted──> Validating ──Connected──> Connected
//!                                        │
//!                                        └── SessionDead / BridgeExited ──> ValidationFailed
//!                                                                              │ (arquiva)
//!                                                                              v
//!                                                                          QrRequired
//!
//!   Connected ──Disconnected{will_retry}──> Reconnecting ──Connected──> Connected
//!   qualquer  ──SessionDead──> SessionDead (apaga a sessao, zera a intencao)
//! ```

/// Teto de QRs por execucao. Depois disso o comando desiste com instrucao
/// explicita em vez de ficar imprimindo QR para sempre.
pub const MAX_QR_ATTEMPTS: u32 = 5;

/// Primeiro degrau do backoff de reconexao (ms).
pub const BACKOFF_MIN_MS: u64 = 1_000;
/// Teto do backoff de reconexao (ms).
pub const BACKOFF_MAX_MS: u64 = 30_000;

/// O que o usuario pediu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desired {
    On,
    Off,
}

/// Por que o vinculo parou de vez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Os 5 QRs expiraram sem leitura.
    QrExpired,
    /// O bridge morreu antes de concluir.
    BridgeGone,
    /// O bridge falou uma versao de protocolo que esta CLI nao entende.
    ProtocolMismatch,
}

/// Onde o vinculo esta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    NotConnected,
    /// Ha blob em disco; ainda nao sabemos se o servidor o aceita.
    SessionFound,
    /// Blob enviado ao bridge, esperando veredito.
    Validating,
    /// O servidor recusou a sessao: ela sera arquivada e o QR volta.
    ValidationFailed,
    QrRequired,
    QrGenerated {
        attempt: u32,
        expires_at_secs: u64,
    },
    WaitingScan {
        attempt: u32,
        expires_at_secs: u64,
    },
    Authenticated,
    Connected,
    Reconnecting {
        attempt: u32,
        retry_at_secs: u64,
    },
    /// A sessao morreu (401/403/419). Um QR novo passa a ser obrigatorio.
    SessionDead,
    Failed(Failure),
    /// `Desired::Off` foi pedido; nada mais avança.
    Stopped,
}

impl Phase {
    /// Estado terminal: o driver pode parar de bombear eventos.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Phase::Connected | Phase::SessionDead | Phase::Failed(_) | Phase::Stopped
        )
    }
}

/// Fatos vindos do bridge (ou do disco, no caso de [`Event::SessionFound`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Ha blob em disco.
    SessionFound,
    /// Nao ha blob em disco.
    NoSession,
    /// `started` com protocolo compativel.
    BridgeStarted,
    /// `started` com protocolo incompativel.
    ProtocolMismatch,
    /// Evento `qr`.
    QrShown { expires_in_secs: u64 },
    /// `status{state:"waiting_scan"}`.
    ScanPending,
    /// Evento `authenticated`.
    Authenticated,
    /// Evento `connected`.
    Connected,
    /// Evento `disconnected`.
    Disconnected { will_retry: bool },
    /// A sessao morreu: o bridge saiu com [`super::protocol::exit_code::
    /// SESSION_DEAD`] (401/403/419). **So o codigo de saida prova isso** — o
    /// evento `logged_out` sozinho nao, porque ele tambem sai num logout que
    /// nos mesmos pedimos e num 440 (`connectionReplaced`), e nesses dois a
    /// sessao continua valendo.
    SessionDead,
    /// O processo do bridge terminou.
    BridgeExited,
    /// O usuario pediu para parar (Ctrl+C).
    Stop,
}

/// O que o driver precisa fazer por causa de uma transicao.
///
/// A maquina nao executa nada: ela **descreve**. Quem toca disco e processo e
/// o driver (`runner.rs`), e e por isso que esta tabela e testavel inteira.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Renderizar o QR que acabou de chegar (tentativa N de [`MAX_QR_ATTEMPTS`]).
    ShowQr { attempt: u32 },
    /// O QR da tela expirou; um novo vem a seguir.
    QrExpired { attempt: u32 },
    /// Mover `session.enc` para `session.enc.prev` antes de parear de novo.
    ArchiveSession,
    /// Apagar o material de sessao: ela nao vale mais.
    PurgeSession,
    /// Esperar `delay_ms` e reconectar. Quem tem o relogio espera.
    ScheduleReconnect { attempt: u32, delay_ms: u64 },
    /// Encerrar com falha.
    Fail(Failure),
    /// Vinculo concluido: pode persistir e gravar `enabled = true`.
    Ready,
}

/// Estado do vinculo.
#[derive(Debug, Clone)]
pub struct Machine {
    desired: Desired,
    phase: Phase,
    qr_attempts: u32,
    reconnect_attempts: u32,
    max_qr_attempts: u32,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self {
            desired: Desired::On,
            phase: Phase::NotConnected,
            qr_attempts: 0,
            reconnect_attempts: 0,
            max_qr_attempts: MAX_QR_ATTEMPTS,
        }
    }

    /// Variante para teste: teto de QR menor deixa a tabela curta.
    pub fn with_max_qr_attempts(mut self, max: u32) -> Self {
        self.max_qr_attempts = max.max(1);
        self
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn desired(&self) -> Desired {
        self.desired
    }

    pub fn qr_attempts(&self) -> u32 {
        self.qr_attempts
    }

    pub fn reconnect_attempts(&self) -> u32 {
        self.reconnect_attempts
    }

    /// Aplica um fato. `now_secs` e injetado — a maquina nunca le relogio.
    pub fn on(&mut self, event: Event, now_secs: u64) -> Vec<Effect> {
        // `Stop` vence tudo, inclusive um evento que chegou junto: a intencao
        // do usuario nao pode perder uma corrida com o bridge.
        if event == Event::Stop {
            self.desired = Desired::Off;
            self.phase = Phase::Stopped;
            return Vec::new();
        }
        if self.desired == Desired::Off {
            // Eventos atrasados depois do Ctrl+C sao ignorados de proposito.
            return Vec::new();
        }
        if matches!(self.phase, Phase::Failed(_) | Phase::SessionDead) {
            return Vec::new();
        }

        match (self.phase, event) {
            // --- descoberta --------------------------------------------------
            (Phase::NotConnected, Event::SessionFound) => {
                self.phase = Phase::SessionFound;
                Vec::new()
            }
            (Phase::NotConnected, Event::NoSession) => {
                self.phase = Phase::QrRequired;
                Vec::new()
            }
            (Phase::SessionFound, Event::BridgeStarted) => {
                self.phase = Phase::Validating;
                Vec::new()
            }
            (Phase::NotConnected | Phase::QrRequired, Event::BridgeStarted) => {
                self.phase = Phase::QrRequired;
                Vec::new()
            }

            // --- protocolo ---------------------------------------------------
            (_, Event::ProtocolMismatch) => {
                self.phase = Phase::Failed(Failure::ProtocolMismatch);
                vec![Effect::Fail(Failure::ProtocolMismatch)]
            }

            // --- validacao de sessao existente -------------------------------
            (Phase::Validating, Event::Connected) => {
                self.phase = Phase::Connected;
                vec![Effect::Ready]
            }
            // A sessao existe mas o servidor a recusou: arquiva e cai no QR.
            // Um `qr` durante a validacao E o veredito negativo — o bridge so
            // pede QR quando as credenciais nao servem.
            (Phase::Validating, Event::QrShown { expires_in_secs }) => {
                self.phase = Phase::ValidationFailed;
                let mut out = vec![Effect::ArchiveSession];
                out.extend(self.enter_qr(expires_in_secs, now_secs));
                out
            }
            (Phase::Validating, Event::BridgeExited) => {
                self.phase = Phase::Failed(Failure::BridgeGone);
                vec![Effect::Fail(Failure::BridgeGone)]
            }

            // --- fluxo de QR --------------------------------------------------
            (
                Phase::QrRequired
                | Phase::ValidationFailed
                | Phase::QrGenerated { .. }
                | Phase::WaitingScan { .. },
                Event::QrShown { expires_in_secs },
            ) => self.enter_qr(expires_in_secs, now_secs),

            (
                Phase::QrGenerated {
                    attempt,
                    expires_at_secs,
                },
                Event::ScanPending,
            ) => {
                self.phase = Phase::WaitingScan {
                    attempt,
                    expires_at_secs,
                };
                Vec::new()
            }

            (Phase::QrGenerated { .. } | Phase::WaitingScan { .. }, Event::Authenticated)
            | (Phase::Validating, Event::Authenticated) => {
                self.phase = Phase::Authenticated;
                Vec::new()
            }

            (Phase::Authenticated, Event::Connected) => {
                self.phase = Phase::Connected;
                vec![Effect::Ready]
            }
            // O bridge pode pular `authenticated` (ordem nao garantida em
            // reconexao pos-pareamento, codigo 515).
            (Phase::QrGenerated { .. } | Phase::WaitingScan { .. }, Event::Connected) => {
                self.phase = Phase::Connected;
                vec![Effect::Ready]
            }

            (Phase::QrGenerated { .. } | Phase::WaitingScan { .. }, Event::BridgeExited) => {
                self.phase = Phase::Failed(Failure::BridgeGone);
                vec![Effect::Fail(Failure::BridgeGone)]
            }

            // --- sessao morta -------------------------------------------------
            (_, Event::SessionDead) => {
                self.phase = Phase::SessionDead;
                // Zera a intencao: uma sessao morta nao deve ser reconectada
                // sozinha. O usuario roda `garra whatsapp` de novo.
                self.desired = Desired::Off;
                vec![Effect::PurgeSession]
            }

            // --- reconexao (modo serve) ---------------------------------------
            (Phase::Connected, Event::Disconnected { will_retry: true }) => {
                self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
                let delay = backoff_ms(self.reconnect_attempts, 0.0);
                self.phase = Phase::Reconnecting {
                    attempt: self.reconnect_attempts,
                    retry_at_secs: now_secs + delay.div_ceil(1000),
                };
                vec![Effect::ScheduleReconnect {
                    attempt: self.reconnect_attempts,
                    delay_ms: delay,
                }]
            }
            (Phase::Connected, Event::Disconnected { will_retry: false }) => {
                self.phase = Phase::Failed(Failure::BridgeGone);
                vec![Effect::Fail(Failure::BridgeGone)]
            }
            (Phase::Reconnecting { .. }, Event::Connected) => {
                self.reconnect_attempts = 0;
                self.phase = Phase::Connected;
                vec![Effect::Ready]
            }
            (Phase::Reconnecting { .. }, Event::Disconnected { will_retry: true }) => {
                self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
                let delay = backoff_ms(self.reconnect_attempts, 0.0);
                self.phase = Phase::Reconnecting {
                    attempt: self.reconnect_attempts,
                    retry_at_secs: now_secs + delay.div_ceil(1000),
                };
                vec![Effect::ScheduleReconnect {
                    attempt: self.reconnect_attempts,
                    delay_ms: delay,
                }]
            }
            (Phase::Connected | Phase::Reconnecting { .. }, Event::BridgeExited) => {
                self.phase = Phase::Failed(Failure::BridgeGone);
                vec![Effect::Fail(Failure::BridgeGone)]
            }

            // Qualquer par que nao esteja na tabela e no-op: um evento fora de
            // ordem nao pode derrubar o vinculo nem inventar transicao.
            _ => Vec::new(),
        }
    }

    /// Passagem de tempo. So o QR expira por tick; o backoff e esperado pelo
    /// driver, que ja recebeu o intervalo em [`Effect::ScheduleReconnect`].
    pub fn tick(&mut self, now_secs: u64) -> Vec<Effect> {
        if self.desired == Desired::Off {
            return Vec::new();
        }
        let (attempt, expires_at_secs) = match self.phase {
            Phase::QrGenerated {
                attempt,
                expires_at_secs,
            }
            | Phase::WaitingScan {
                attempt,
                expires_at_secs,
            } => (attempt, expires_at_secs),
            _ => return Vec::new(),
        };
        if now_secs < expires_at_secs {
            return Vec::new();
        }

        if attempt >= self.max_qr_attempts {
            self.phase = Phase::Failed(Failure::QrExpired);
            return vec![
                Effect::QrExpired { attempt },
                Effect::Fail(Failure::QrExpired),
            ];
        }
        // Volta para `QrRequired`: o proximo `qr` do bridge conta como a
        // tentativa seguinte. A maquina nao pede o QR — ela so deixa de
        // esperar por este.
        self.phase = Phase::QrRequired;
        vec![Effect::QrExpired { attempt }]
    }

    fn enter_qr(&mut self, expires_in_secs: u64, now_secs: u64) -> Vec<Effect> {
        if self.qr_attempts >= self.max_qr_attempts {
            self.phase = Phase::Failed(Failure::QrExpired);
            return vec![Effect::Fail(Failure::QrExpired)];
        }
        self.qr_attempts = self.qr_attempts.saturating_add(1);
        let expires_in = if expires_in_secs == 0 {
            super::protocol::DEFAULT_QR_EXPIRY_SECS
        } else {
            expires_in_secs
        };
        self.phase = Phase::QrGenerated {
            attempt: self.qr_attempts,
            expires_at_secs: now_secs.saturating_add(expires_in),
        };
        vec![Effect::ShowQr {
            attempt: self.qr_attempts,
        }]
    }
}

/// Backoff de reconexao: 1 s dobrando ate 30 s, com **equal jitter**.
///
/// `jitter` vem de fora, em `[0.0, 1.0]` — a maquina nao sorteia nada, pelo
/// mesmo motivo de nao ler relogio. O resultado fica em
/// `[base/2, base]`: metade fixa garante que a reconexao nao vira busy-loop,
/// metade aleatoria evita que N instancias reconectem no mesmo milissegundo.
pub fn backoff_ms(attempt: u32, jitter: f64) -> u64 {
    let exponent = attempt.saturating_sub(1).min(16);
    let base = BACKOFF_MIN_MS
        .saturating_mul(1u64 << exponent)
        .min(BACKOFF_MAX_MS);
    let half = base / 2;
    let jitter = jitter.clamp(0.0, 1.0);
    half + (half as f64 * jitter) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qr(secs: u64) -> Event {
        Event::QrShown {
            expires_in_secs: secs,
        }
    }

    #[test]
    fn happy_path_without_a_session() {
        let mut m = Machine::new();
        assert!(m.on(Event::NoSession, 0).is_empty());
        assert_eq!(m.phase(), Phase::QrRequired);
        assert_eq!(m.on(qr(20), 0), vec![Effect::ShowQr { attempt: 1 }]);
        assert!(m.on(Event::ScanPending, 1).is_empty());
        assert!(matches!(m.phase(), Phase::WaitingScan { attempt: 1, .. }));
        assert!(m.on(Event::Authenticated, 5).is_empty());
        assert_eq!(m.phase(), Phase::Authenticated);
        assert_eq!(m.on(Event::Connected, 6), vec![Effect::Ready]);
        assert!(m.phase().is_terminal());
    }

    #[test]
    fn existing_valid_session_never_asks_for_a_qr() {
        let mut m = Machine::new();
        m.on(Event::SessionFound, 0);
        m.on(Event::BridgeStarted, 0);
        assert_eq!(m.phase(), Phase::Validating);
        assert_eq!(m.on(Event::Connected, 1), vec![Effect::Ready]);
        assert_eq!(m.qr_attempts(), 0, "nenhum QR pode ter sido pedido");
    }

    #[test]
    fn a_qr_during_validation_archives_the_old_blob_first() {
        let mut m = Machine::new();
        m.on(Event::SessionFound, 0);
        m.on(Event::BridgeStarted, 0);
        let effects = m.on(qr(20), 3);
        assert_eq!(
            effects,
            vec![Effect::ArchiveSession, Effect::ShowQr { attempt: 1 }],
            "arquivar precisa vir ANTES de desenhar o QR novo"
        );
    }

    /// A tabela do teto: cada expiracao regenera ate o quinto QR, e a quinta
    /// expiracao falha.
    #[test]
    fn qr_expiry_regenerates_up_to_the_cap() {
        let mut m = Machine::new();
        m.on(Event::NoSession, 0);

        let mut now = 0u64;
        for attempt in 1..=MAX_QR_ATTEMPTS {
            assert_eq!(m.on(qr(20), now), vec![Effect::ShowQr { attempt }]);
            now += 20;
            let effects = m.tick(now);
            if attempt < MAX_QR_ATTEMPTS {
                assert_eq!(effects, vec![Effect::QrExpired { attempt }]);
                assert_eq!(m.phase(), Phase::QrRequired);
            } else {
                assert_eq!(
                    effects,
                    vec![
                        Effect::QrExpired { attempt },
                        Effect::Fail(Failure::QrExpired)
                    ]
                );
                assert_eq!(m.phase(), Phase::Failed(Failure::QrExpired));
            }
        }
    }

    #[test]
    fn tick_before_expiry_does_nothing() {
        let mut m = Machine::new();
        m.on(Event::NoSession, 0);
        m.on(qr(20), 100);
        assert!(m.tick(119).is_empty());
        assert!(matches!(m.phase(), Phase::QrGenerated { attempt: 1, .. }));
        assert!(!m.tick(120).is_empty());
    }

    #[test]
    fn a_sixth_qr_is_refused_even_without_a_tick() {
        let mut m = Machine::new();
        m.on(Event::NoSession, 0);
        for attempt in 1..=MAX_QR_ATTEMPTS {
            assert_eq!(m.on(qr(20), 0), vec![Effect::ShowQr { attempt }]);
        }
        assert_eq!(m.on(qr(20), 0), vec![Effect::Fail(Failure::QrExpired)]);
    }

    #[test]
    fn a_dead_session_purges_and_clears_the_intent_from_any_phase() {
        for setup in [
            vec![
                Event::NoSession,
                Event::QrShown {
                    expires_in_secs: 20,
                },
            ],
            vec![Event::SessionFound, Event::BridgeStarted],
            vec![
                Event::NoSession,
                Event::QrShown {
                    expires_in_secs: 20,
                },
                Event::Connected,
            ],
        ] {
            let mut m = Machine::new();
            for ev in setup {
                m.on(ev, 0);
            }
            assert_eq!(m.on(Event::SessionDead, 1), vec![Effect::PurgeSession]);
            assert_eq!(m.phase(), Phase::SessionDead);
            assert_eq!(m.desired(), Desired::Off);
            assert!(
                m.on(Event::Connected, 2).is_empty(),
                "uma sessao morta nao volta sozinha"
            );
        }
    }

    #[test]
    fn desired_off_never_reaches_connected() {
        let mut m = Machine::new();
        m.on(Event::NoSession, 0);
        m.on(qr(20), 0);
        m.on(Event::Stop, 1);
        assert_eq!(m.desired(), Desired::Off);
        assert_eq!(m.phase(), Phase::Stopped);

        // Todo evento que normalmente levaria a Connected, um a um.
        for ev in [
            Event::Authenticated,
            Event::Connected,
            Event::QrShown {
                expires_in_secs: 20,
            },
            Event::ScanPending,
            Event::BridgeStarted,
        ] {
            assert!(m.on(ev, 2).is_empty(), "{ev:?} nao pode produzir efeito");
            assert_eq!(m.phase(), Phase::Stopped, "{ev:?} mudou a fase");
        }
        assert!(m.tick(1_000).is_empty());
    }

    #[test]
    fn stop_wins_even_from_connected() {
        let mut m = Machine::new();
        m.on(Event::NoSession, 0);
        m.on(qr(20), 0);
        m.on(Event::Connected, 1);
        assert_eq!(m.phase(), Phase::Connected);
        m.on(Event::Stop, 2);
        assert_eq!(m.phase(), Phase::Stopped);
    }

    #[test]
    fn protocol_mismatch_fails_immediately() {
        let mut m = Machine::new();
        assert_eq!(
            m.on(Event::ProtocolMismatch, 0),
            vec![Effect::Fail(Failure::ProtocolMismatch)]
        );
        assert_eq!(m.phase(), Phase::Failed(Failure::ProtocolMismatch));
    }

    #[test]
    fn bridge_death_before_connecting_is_a_failure() {
        let mut m = Machine::new();
        m.on(Event::NoSession, 0);
        m.on(qr(20), 0);
        assert_eq!(
            m.on(Event::BridgeExited, 1),
            vec![Effect::Fail(Failure::BridgeGone)]
        );
    }

    #[test]
    fn reconnect_escalates_then_resets_on_success() {
        let mut m = Machine::new();
        m.on(Event::NoSession, 0);
        m.on(qr(20), 0);
        m.on(Event::Connected, 1);

        let mut now = 1;
        let mut delays = Vec::new();
        for _ in 0..4 {
            let effects = m.on(Event::Disconnected { will_retry: true }, now);
            match effects.as_slice() {
                [Effect::ScheduleReconnect { delay_ms, .. }] => delays.push(*delay_ms),
                other => panic!("esperava ScheduleReconnect, veio {other:?}"),
            }
            now += 60;
        }
        assert_eq!(delays, vec![500, 1_000, 2_000, 4_000], "equal jitter, j=0");
        assert_eq!(m.reconnect_attempts(), 4);

        assert_eq!(m.on(Event::Connected, now), vec![Effect::Ready]);
        assert_eq!(m.reconnect_attempts(), 0, "sucesso zera a escada");
    }

    #[test]
    fn a_disconnect_that_will_not_retry_fails() {
        let mut m = Machine::new();
        m.on(Event::NoSession, 0);
        m.on(qr(20), 0);
        m.on(Event::Connected, 1);
        assert_eq!(
            m.on(Event::Disconnected { will_retry: false }, 2),
            vec![Effect::Fail(Failure::BridgeGone)]
        );
    }

    /// Tabela do backoff: degrau, jitter, intervalo esperado.
    #[test]
    fn backoff_table() {
        let cases: &[(u32, f64, u64)] = &[
            (1, 0.0, 500),
            (1, 1.0, 1_000),
            (1, 0.5, 750),
            (2, 0.0, 1_000),
            (3, 0.0, 2_000),
            (5, 0.0, 8_000),
            (6, 0.0, 15_000), // base saturou no teto de 30 s
            (7, 0.0, 15_000),
            (30, 0.0, 15_000), // saturacao nao estoura
            (30, 1.0, 30_000),
            // jitter fora da faixa e apertado, nao aceito
            (1, -5.0, 500),
            (1, 9.0, 1_000),
        ];
        for &(attempt, jitter, expected) in cases {
            assert_eq!(
                backoff_ms(attempt, jitter),
                expected,
                "attempt={attempt} jitter={jitter}"
            );
        }
    }

    #[test]
    fn backoff_never_exceeds_the_ceiling() {
        for attempt in 1..200 {
            assert!(backoff_ms(attempt, 1.0) <= BACKOFF_MAX_MS);
            assert!(backoff_ms(attempt, 0.0) >= BACKOFF_MIN_MS / 2);
        }
    }

    #[test]
    fn out_of_order_events_are_ignored_not_fatal() {
        let mut m = Machine::new();
        // `authenticated` antes de qualquer QR: sem transicao, sem panico.
        assert!(m.on(Event::Authenticated, 0).is_empty());
        assert_eq!(m.phase(), Phase::NotConnected);
        assert!(m.on(Event::ScanPending, 0).is_empty());
        assert_eq!(m.phase(), Phase::NotConnected);
    }

    #[test]
    fn a_zero_expiry_falls_back_to_the_protocol_default() {
        let mut m = Machine::new();
        m.on(Event::NoSession, 0);
        m.on(qr(0), 100);
        match m.phase() {
            Phase::QrGenerated {
                expires_at_secs, ..
            } => assert_eq!(
                expires_at_secs,
                100 + super::super::protocol::DEFAULT_QR_EXPIRY_SECS
            ),
            other => panic!("esperava QrGenerated, veio {other:?}"),
        }
    }
}
