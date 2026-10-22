//! #1650: guarda de cadencia ENTRE turnos, por chat.
//!
//! O detector de loop do runtime so enxerga repeticao *dentro* de um turno.
//! Um DM que vira ping-pong com outro agente (auto-resposta) chega aqui como
//! mensagens separadas a cada poucos segundos, e cada uma e uma chamada paga
//! ao provider. Esta guarda corta o turno quando um chat passa do teto de
//! turnos dentro da janela.
//!
//! Modulo puro: o instante entra por parametro (nada de relogio aqui), nada de
//! I/O, nada de identidade — a chave e o `chat_jid` e so ela. Estado em
//! memoria; um restart zera, de proposito (fail-safe, nao ledger).
//!
//! O que este modulo entrega alem da primeira fatia (#1658):
//!
//! - **Teto configuravel**: [`Cadencia::set_max`] recebe o knob do operador
//!   (`channels.whatsapp_linked.settings.max_turnos_per_minute`); ausente,
//!   zero ou invalido vale o default de sempre ([`MAX_TURNOS_POR_JANELA`]
//!   = 20/60s).
//! - **Backoff crescente**: a n-th rajada consecutiva do mesmo chat segura a
//!   volta em `janela * 2^(n-1)` (60s, 120s, 240s, teto 480s). Silencio de
//!   [`GRACE_POR_RAJADA`] zera a sequencia — humano erra o teto uma vez e
//!   volta; bot insiste e afunda sozinho.
//! - **Conteudo suspeito com teto menor**: mensagem vazia ou marcador de turno
//!   vazio (o `[no textual response provided by the model]` que o incidente
//!   repetiu) e sinal de loop, nao de conversa: essas admitem no maximo
//!   [`MAX_CONTEUDO_SUSPEITO`] por janela, mesmo quando o teto do operador e
//!   maior.
//! - **Diagnostico por hora**: [`Cadencia::diagnosticar`] devolve
//!   `turnos_ultima_hora` (janela de 1h dos admitidos) para a tool de
//!   diagnostico do gateway — o `garraia doctor` offline nao enxerga isto
//!   porque e outro processo, e o PR diz isso com todas as letras.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Teto padrao conservador: bem acima de uma conversa humana, bem abaixo do
/// incidente (um turno a cada 2-10s sustentado).
pub const MAX_TURNOS_POR_JANELA: usize = 20;
/// Janela deslizante do teto.
pub const JANELA: Duration = Duration::from_secs(60);
/// Teto por janela quando o conteudo chega vazio ou como marcador de turno
/// vazio — abaixo do teto normal de proposito: humano nao manda 3 vazias
/// seguidas, loop manda.
pub const MAX_CONTEUDO_SUSPEITO: usize = 3;
/// O marcador interno que o caminho nao-streaming ja publicava como resposta
/// (ver #1648/#1651): recebido de volta como mensagem inbound, e stub de bot.
pub const STUB_MARCADOR: &str = "[no textual response provided by the model]";
/// Silencio que quebra a sequencia de rajadas (zera o backoff). Precisa ser
/// MAIOR que o maior backoff (480s): se o grace expirasse junto com o
/// bloqueio, o primeiro admitido pós-bloqueio zeraria a sequencia e o bot
/// persistente nunca subiria de patamar.
pub const GRACE_POR_RAJADA: Duration = Duration::from_secs(300);
/// Janela de contagem do `turnos_ultima_hora` do diagnostico.
pub const JANELA_HORA: Duration = Duration::from_secs(3600);
/// Exponente maximo do backoff: `JANELA * 2^EXP_MAX` = 8 minutos.
const EXP_MAX: u32 = 3;
/// Teto de chats lembrados; passando disso, os ociosos sao descartados.
const CHATS_MAX: usize = 4096;

/// O que a mensagem inbound traz de conteuto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conteudo {
    /// Texto normal.
    Texto,
    /// Vazia depois do trim.
    Vazio,
    /// O marcador de turno vazio — assinatura de outro agente/publicador.
    Stub,
}

/// Classifica o texto bruto da mensagem. O trim e daqui: quem chama entrega o
/// bruto, e a politica nao pode variar com espaco.
pub fn classifica(texto: &str) -> Conteudo {
    let t = texto.trim();
    if t.is_empty() {
        Conteudo::Vazio
    } else if t.contains(STUB_MARCADOR) {
        Conteudo::Stub
    } else {
        Conteudo::Texto
    }
}

/// O que fazer com o turno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decisao {
    /// Dentro do teto: segue para o modelo.
    Admitido,
    /// Estourou o teto ou esta no backoff. `turnos` = quantos ja ocorreram na
    /// janela. `primeiro_corte` e `true` so no primeiro corte da rajada: e
    /// quando vale avisar uma unica vez, em vez de responder a cada corte.
    Cortado { turnos: usize, primeiro_corte: bool },
}

/// Instantaneo para o diagnostico do gateway (tool `garra_diagnostics` e
/// `GET /api/diagnostics`): contagem, nunca identidade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagDeCadencia {
    /// Turnos admitidos nas ultimas [`JANELA_HORA`], somando todos os chats.
    pub turnos_ultima_hora: usize,
    /// Chats com estado vivo na janela de teto.
    pub chats_ativos: usize,
    /// Teto efetivo apos o knob do operador.
    pub max_por_janela: usize,
    /// Janela do teto em segundos.
    pub janela_s: u64,
}

#[derive(Debug)]
pub struct Cadencia {
    max: usize,
    /// O ultimo teto que VEIO DA CONFIG (para `set_max(None)` saber se deve
    /// desfazer um declarado ou ignorar um construido).
    max_operador: Option<usize>,
    janela: Duration,
    turnos: HashMap<String, VecDeque<Instant>>,
    /// Chats em corte desde o ultimo `Admitido` (para avisar uma vez so).
    em_corte: HashMap<String, ()>,
    /// Backoff: ate quando o chat esta segurado apos a ultima rajada.
    bloqueio_ate: HashMap<String, Instant>,
    /// Rajadas consecutivas sem grace no meio (alimenta o expoente).
    rajadas: HashMap<String, u32>,
    /// Ultima vez que o chat estourou (para medir o grace).
    ultimo_corte: HashMap<String, Instant>,
    /// Todos os admitidos na ultima hora (janela do diagnostico).
    por_hora: VecDeque<Instant>,
}

impl Default for Cadencia {
    fn default() -> Self {
        Self::new(MAX_TURNOS_POR_JANELA, JANELA)
    }
}

impl Cadencia {
    pub fn new(max: usize, janela: Duration) -> Self {
        Self {
            max: max.max(1),
            max_operador: None,
            janela,
            turnos: HashMap::new(),
            em_corte: HashMap::new(),
            bloqueio_ate: HashMap::new(),
            rajadas: HashMap::new(),
            ultimo_corte: HashMap::new(),
            por_hora: VecDeque::new(),
        }
    }

    /// Teto vindo do operador (`settings.max_turnos_per_minute`).
    ///
    /// - `Some(v)` valido → vale v e fica marcado como declarado.
    /// - `None` **declarado** → volta ao default (chave removida da config:
    ///   hot-reload desfaz o que a chave fez).
    /// - `None` **na primeira chamada com teto de construcao** (default ou o
    ///   teto generoso que os testes injetam) → **nao toca**: sem a chave na
    ///   config, quem decide e o construtor. Sem essa regra, cada turno
    ///   derrubaria o teto do `com_cadencia` para 20 e o teste de politica
    ///   perdia a 21a mensagem (regressao real desta fatia, capturada pelo
    ///   gate).
    pub fn set_max(&mut self, operador: Option<u32>) {
        match operador {
            Some(v) if v > 0 => {
                self.max = (v as usize).max(1);
                self.max_operador = Some(self.max);
            }
            Some(_) => {
                // Zero = valor que o operador nao quis escrever de verdade:
                // nao vira teto-1; se a chave existir, volta ao default.
                if self.max_operador.is_some() {
                    self.max = MAX_TURNOS_POR_JANELA;
                    self.max_operador = None;
                }
            }
            None => {
                if self.max_operador.is_some() {
                    self.max = MAX_TURNOS_POR_JANELA;
                    self.max_operador = None;
                }
                // None na primeira chamada: nao toca (teto do construtor).
            }
        }
    }

    /// Decide e registra. Turno cortado **nao** conta na janela: a tempestade
    /// nao se auto-prolonga, e quando o outro lado cala o chat volta sozinho.
    pub fn admitir(&mut self, chat: &str, agora: Instant, conteudo: Conteudo) -> Decisao {
        if self.turnos.len() >= CHATS_MAX && !self.turnos.contains_key(chat) {
            self.varrer(agora);
        }
        // Backoff: segurado por uma rajada anterior, o chat volta quando o
        // bloqueio expira — mesmo que a janela pura ja tivesse liberado.
        if self.bloqueio_ate.get(chat).is_some_and(|ate| agora < *ate) {
            let turnos = self.turnos.get(chat).map_or(0, |f| f.len());
            return Decisao::Cortado {
                turnos,
                primeiro_corte: false,
            };
        }
        let fila = self.turnos.entry(chat.to_string()).or_default();
        while fila
            .front()
            .is_some_and(|t| agora.saturating_duration_since(*t) >= self.janela)
        {
            fila.pop_front();
        }
        let limite = match conteudo {
            Conteudo::Texto => self.max,
            Conteudo::Vazio | Conteudo::Stub => self.max.min(MAX_CONTEUDO_SUSPEITO),
        };
        if fila.len() >= limite {
            let turnos = fila.len();
            let primeiro_corte = self.em_corte.insert(chat.to_string(), ()).is_none();
            if primeiro_corte {
                // Nova rajada: cresce o backoff (2^(n-1), teto EXP_MAX) e
                // marca o instante para o grace medir depois. Rajada 1 NAO
                // recebe bloqueio explicito: a propria janela ja segura 60s,
                // e e o contrato da primeira fatia (#1658) — o backoff
                // explicito comeca na 2a rajada.
                let n = self.rajadas.entry(chat.to_string()).or_insert(0);
                *n = n.saturating_add(1);
                let exp = (*n - 1).min(EXP_MAX);
                if exp >= 1 {
                    let segs = self.janela.as_secs().saturating_mul(1u64 << exp);
                    self.bloqueio_ate
                        .insert(chat.to_string(), agora + Duration::from_secs(segs));
                }
                self.ultimo_corte.insert(chat.to_string(), agora);
            }
            return Decisao::Cortado {
                turnos,
                primeiro_corte,
            };
        }
        fila.push_back(agora);
        self.em_corte.remove(chat);
        // Diagnostico: janela de 1h dos admitidos (global — contagem, nunca
        // identidade).
        self.por_hora.push_back(agora);
        while self
            .por_hora
            .front()
            .is_some_and(|t| agora.saturating_duration_since(*t) >= JANELA_HORA)
        {
            self.por_hora.pop_front();
        }
        // Grace: silencio longo o bastante zera a sequencia de rajadas.
        if self
            .ultimo_corte
            .get(chat)
            .is_some_and(|uc| agora.saturating_duration_since(*uc) >= GRACE_POR_RAJADA)
        {
            self.rajadas.remove(chat);
            self.bloqueio_ate.remove(chat);
        }
        Decisao::Admitido
    }

    /// Turnos admitidos na janela corrente de teto, para um chat (testes e
    /// depuracao manual — nao e superficie de produto).
    #[cfg(test)]
    pub fn turnos_na_janela(&self, chat: &str, agora: Instant) -> usize {
        self.turnos.get(chat).map_or(0, |f| {
            f.iter()
                .filter(|t| agora.saturating_duration_since(**t) < self.janela)
                .count()
        })
    }

    /// Instantaneo para o diagnostico do gateway.
    pub fn diagnosticar(&self, agora: Instant) -> DiagDeCadencia {
        let turnos_ultima_hora = self
            .por_hora
            .iter()
            .filter(|t| agora.saturating_duration_since(**t) < JANELA_HORA)
            .count();
        DiagDeCadencia {
            turnos_ultima_hora,
            chats_ativos: self.turnos.len(),
            max_por_janela: self.max,
            janela_s: self.janela.as_secs(),
        }
    }

    fn varrer(&mut self, agora: Instant) {
        let janela = self.janela;
        self.turnos.retain(|_, f| {
            f.back()
                .is_some_and(|t| agora.saturating_duration_since(*t) < janela)
        });
        let vivos = &self.turnos;
        self.em_corte.retain(|c, _| vivos.contains_key(c));
        self.bloqueio_ate
            .retain(|c, ate| vivos.contains_key(c) || *ate > agora);
        self.rajadas.retain(|c, _| vivos.contains_key(c));
        self.ultimo_corte.retain(|c, _| vivos.contains_key(c));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAT: &str = "chat-a@s.whatsapp.net";

    fn seg(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn conversa_humana_passa() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(3, seg(60));
        for i in 0..10 {
            assert_eq!(
                c.admitir(CHAT, t0 + seg(i * 30), Conteudo::Texto),
                Decisao::Admitido
            );
        }
    }

    #[test]
    fn rajada_estoura_o_teto_e_avisa_so_uma_vez() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(3, seg(60));
        for i in 0..3 {
            assert_eq!(
                c.admitir(CHAT, t0 + seg(i * 2), Conteudo::Texto),
                Decisao::Admitido
            );
        }
        assert_eq!(
            c.admitir(CHAT, t0 + seg(6), Conteudo::Texto),
            Decisao::Cortado {
                turnos: 3,
                primeiro_corte: true
            }
        );
        assert_eq!(
            c.admitir(CHAT, t0 + seg(8), Conteudo::Texto),
            Decisao::Cortado {
                turnos: 3,
                primeiro_corte: false
            }
        );
    }

    #[test]
    fn corte_nao_se_auto_prolonga() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(2, seg(60));
        assert_eq!(c.admitir(CHAT, t0, Conteudo::Texto), Decisao::Admitido);
        assert_eq!(
            c.admitir(CHAT, t0 + seg(1), Conteudo::Texto),
            Decisao::Admitido
        );
        // Cortados durante a janela nao entram na contagem...
        for i in 2..30 {
            assert!(matches!(
                c.admitir(CHAT, t0 + seg(i), Conteudo::Texto),
                Decisao::Cortado { .. }
            ));
        }
        // ...e 60s depois do primeiro admitido o chat volta: o primeiro
        // backoff e a propria janela (mesmo comportamento da #1658).
        assert_eq!(
            c.admitir(CHAT, t0 + seg(61), Conteudo::Texto),
            Decisao::Admitido
        );
    }

    #[test]
    fn chats_sao_independentes() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(1, seg(60));
        assert_eq!(c.admitir("a", t0, Conteudo::Texto), Decisao::Admitido);
        assert_eq!(c.admitir("b", t0, Conteudo::Texto), Decisao::Admitido);
        assert!(matches!(
            c.admitir("a", t0 + seg(1), Conteudo::Texto),
            Decisao::Cortado { .. }
        ));
    }

    #[test]
    fn novo_corte_depois_de_recuperar_avisa_de_novo() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(1, seg(10));
        c.admitir(CHAT, t0, Conteudo::Texto);
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(1), Conteudo::Texto),
            Decisao::Cortado {
                primeiro_corte: true,
                ..
            }
        ));
        c.admitir(CHAT, t0 + seg(11), Conteudo::Texto);
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(12), Conteudo::Texto),
            Decisao::Cortado {
                primeiro_corte: true,
                ..
            }
        ));
    }

    #[test]
    fn teto_zero_vira_um() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(0, seg(60));
        assert_eq!(c.admitir(CHAT, t0, Conteudo::Texto), Decisao::Admitido);
    }

    #[test]
    fn contagem_da_janela() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(5, seg(60));
        c.admitir(CHAT, t0, Conteudo::Texto);
        c.admitir(CHAT, t0 + seg(30), Conteudo::Texto);
        assert_eq!(c.turnos_na_janela(CHAT, t0 + seg(40)), 2);
        assert_eq!(c.turnos_na_janela(CHAT, t0 + seg(70)), 1);
    }

    // ─── knob do operador ────────────────────────────────────────────────

    #[test]
    fn knob_ausente_mantem_o_default() {
        let mut c = Cadencia::default();
        c.set_max(None);
        let t0 = Instant::now();
        for i in 0..MAX_TURNOS_POR_JANELA {
            assert_eq!(
                c.admitir(CHAT, t0 + seg(i as u64), Conteudo::Texto),
                Decisao::Admitido
            );
        }
        // A 21a DENTRO da janela estoura; fora (t+100) a janela ja esvaziou
        // e o chat volta — e e assim que deve ser.
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(21), Conteudo::Texto),
            Decisao::Cortado { .. }
        ));
        assert_eq!(
            c.admitir(CHAT, t0 + seg(81), Conteudo::Texto),
            Decisao::Admitido,
            "janela esvaziada = volta sozinho (sem bloqueio na rajada 1)"
        );
    }

    #[test]
    fn knob_zero_nao_torna_teto_um() {
        // `Some(0)` = valor que o operador nao quis escrever de verdade:
        // volta pro default (se a chave ja tivesse sido Some antes), nao vira
        // 1/min — e na primeira chamada nao derruba o teto do construtor.
        let mut c = Cadencia::default();
        c.set_max(Some(0));
        let t0 = Instant::now();
        for i in 0..MAX_TURNOS_POR_JANELA {
            assert_eq!(
                c.admitir(CHAT, t0 + seg(i as u64), Conteudo::Texto),
                Decisao::Admitido
            );
        }
    }

    #[test]
    fn set_max_none_nao_derruba_teto_de_construcao() {
        // O cenario do com_cadencia dos testes de politica: construtor com
        // teto generoso + set_max(None) em CADA turno. Se None derrubasse
        // para 20, a 21a mensagem daquele teste morria (foi assim que a
        // regressao apareceu).
        let mut c = Cadencia::new(1000, seg(60));
        let t0 = Instant::now();
        for i in 0..30u64 {
            c.set_max(None);
            assert_eq!(
                c.admitir(CHAT, t0 + seg(i), Conteudo::Texto),
                Decisao::Admitido,
                "teto de construcao sobrevive ao set_max(None) do turno"
            );
        }
    }

    #[test]
    fn set_max_none_desfaz_declarado_anterior() {
        // Hot-reload: chave Some(50) ligou, chave removida (None) volta ao
        // default — o declarado nao gruda para sempre.
        let mut c = Cadencia::default();
        c.set_max(Some(50));
        assert_eq!(c.diagnosticar(Instant::now()).max_por_janela, 50);
        c.set_max(None);
        assert_eq!(
            c.diagnosticar(Instant::now()).max_por_janela,
            MAX_TURNOS_POR_JANELA
        );
    }

    #[test]
    fn knob_personalizado_e_respeitado() {
        let mut c = Cadencia::default();
        c.set_max(Some(4));
        let t0 = Instant::now();
        for i in 0..4 {
            assert_eq!(
                c.admitir(CHAT, t0 + seg(i), Conteudo::Texto),
                Decisao::Admitido
            );
        }
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(10), Conteudo::Texto),
            Decisao::Cortado { .. }
        ));
    }

    // ─── conteudo suspeito ──────────────────────────────────────────────

    #[test]
    fn vazio_e_stub_respeitam_o_teto_menor() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(20, seg(60)); // teto do operador alto
        for i in 0..MAX_CONTEUDO_SUSPEITO {
            assert_eq!(
                c.admitir(CHAT, t0 + seg(i as u64), Conteudo::Stub),
                Decisao::Admitido
            );
        }
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(10), Conteudo::Stub),
            Decisao::Cortado { .. }
        ));
        // E o vazio cai no mesmo teto, por chat.
        let outro = "outro@s.whatsapp.net";
        for i in 0..MAX_CONTEUDO_SUSPEITO {
            assert_eq!(
                c.admitir(outro, t0 + seg(i as u64), Conteudo::Vazio),
                Decisao::Admitido
            );
        }
        assert!(matches!(
            c.admitir(outro, t0 + seg(10), Conteudo::Vazio),
            Decisao::Cortado { .. }
        ));
    }

    #[test]
    fn teto_menor_do_suspeito_nunca_supera_o_do_operador() {
        // Operador com teto 2: suspeito vale 2 (min(2,3)), nao 3.
        let t0 = Instant::now();
        let mut c = Cadencia::new(2, seg(60));
        assert_eq!(c.admitir(CHAT, t0, Conteudo::Vazio), Decisao::Admitido);
        assert_eq!(
            c.admitir(CHAT, t0 + seg(1), Conteudo::Vazio),
            Decisao::Admitido
        );
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(2), Conteudo::Vazio),
            Decisao::Cortado { .. }
        ));
    }

    #[test]
    fn classifica_texto_vazio_e_stub() {
        assert_eq!(classifica("  \n\t "), Conteudo::Vazio);
        assert_eq!(classifica(""), Conteudo::Vazio);
        assert_eq!(
            classifica("[no textual response provided by the model]"),
            Conteudo::Stub
        );
        assert_eq!(
            classifica("prefixo [no textual response provided by the model] sufixo"),
            Conteudo::Stub
        );
        assert_eq!(classifica("oi, tudo bem?"), Conteudo::Texto);
    }

    // ─── backoff crescente ──────────────────────────────────────────────

    #[test]
    fn backoff_cresce_por_rajada() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(1, seg(60));
        // Rajada 1: SEM bloqueio explicito — a janela segura 60s (contrato
        // da #1658, coberto por corte_nao_se_auto_prolonga).
        c.admitir(CHAT, t0, Conteudo::Texto);
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(1), Conteudo::Texto),
            Decisao::Cortado {
                primeiro_corte: true,
                ..
            }
        ));
        assert!(!c.bloqueio_ate.contains_key(CHAT), "rajada 1 = so janela");
        assert_eq!(
            c.admitir(CHAT, t0 + seg(61), Conteudo::Texto),
            Decisao::Admitido
        );
        // Rajada 2 (sem grace no meio): bloqueio explicito de 120s a partir
        // do primeiro corte da rajada (t62) → volta em t182.
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(62), Conteudo::Texto),
            Decisao::Cortado {
                primeiro_corte: true,
                ..
            }
        ));
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(181), Conteudo::Texto),
            Decisao::Cortado {
                primeiro_corte: false,
                ..
            }
        ));
        assert_eq!(
            c.admitir(CHAT, t0 + seg(182), Conteudo::Texto),
            Decisao::Admitido,
            "segundo backoff = 120s a partir do corte"
        );
        // Rajada 3: 240s a partir do corte em t183 → t423.
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(183), Conteudo::Texto),
            Decisao::Cortado {
                primeiro_corte: true,
                ..
            }
        ));
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(400), Conteudo::Texto),
            Decisao::Cortado { .. }
        ));
        assert_eq!(
            c.admitir(CHAT, t0 + seg(423), Conteudo::Texto),
            Decisao::Admitido
        );
        // Rajada 4: expoente 3 = 480s (teto) → t904.
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(424), Conteudo::Texto),
            Decisao::Cortado {
                primeiro_corte: true,
                ..
            }
        ));
        assert_eq!(
            c.rajadas.get(CHAT).copied().unwrap_or(0),
            4,
            "quatro rajadas consecutivas"
        );
        let ate = *c.bloqueio_ate.get(CHAT).expect("bloqueio_ate da rajada 4");
        assert_eq!(
            ate.duration_since(t0 + seg(424)),
            seg(480),
            "teto do backoff = 8x janela"
        );
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(800), Conteudo::Texto),
            Decisao::Cortado { .. }
        ));
        assert_eq!(
            c.admitir(CHAT, t0 + seg(904), Conteudo::Texto),
            Decisao::Admitido
        );
    }

    #[test]
    fn backoff_tem_teto_de_oito_minutos() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(1, seg(60));
        // Simula a 4a rajada (expoente 3 = 480s) direto no estado interno —
        // percorrer 3 rajadas reais so pra isso tornaria o teste clock-heavy.
        c.rajadas.insert(CHAT.to_string(), 4);
        c.bloqueio_ate.insert(CHAT.to_string(), t0 + seg(480));
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(479), Conteudo::Texto),
            Decisao::Cortado {
                primeiro_corte: false,
                ..
            }
        ));
        assert_eq!(
            c.admitir(CHAT, t0 + seg(480), Conteudo::Texto),
            Decisao::Admitido,
            "expirou o teto de 8x janela"
        );
    }

    #[test]
    fn grace_zera_a_sequencia() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(1, seg(60));
        c.admitir(CHAT, t0, Conteudo::Texto);
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(1), Conteudo::Texto),
            Decisao::Cortado { .. }
        ));
        c.admitir(CHAT, t0 + seg(61), Conteudo::Texto);
        // Rajada 2 cria o bloqueio de 120s (t62 → t182).
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(62), Conteudo::Texto),
            Decisao::Cortado {
                primeiro_corte: true,
                ..
            }
        ));
        assert_eq!(
            c.admitir(CHAT, t0 + seg(182), Conteudo::Texto),
            Decisao::Admitido
        );
        // Grace (300s > maior backoff): o admitido no fim do bloqueio NAO
        // zera a sequencia (delta 120 < 300) — sem isso o bot persistente
        // recomeçaria sempre em 60s.
        assert_eq!(c.rajadas.get(CHAT).copied().unwrap_or(0), 2);
        // Silencio de verdade: admitido com delta >= 300 do ultimo corte
        // zera a sequencia.
        c.admitir(CHAT, t0 + seg(400), Conteudo::Texto); // delta 338 ≥ 300
        assert_eq!(
            c.rajadas.get(CHAT).copied().unwrap_or(0),
            0,
            "grace zera a sequencia de rajadas"
        );
        // Proxima rajada recomeca COMO rajada-1: sem bloqueio explicito.
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(401), Conteudo::Texto),
            Decisao::Cortado {
                primeiro_corte: true,
                ..
            }
        ));
        assert!(
            !c.bloqueio_ate.contains_key(CHAT),
            "apos grace, rajada 1 volta a valer so a janela"
        );
        assert_eq!(
            c.admitir(CHAT, t0 + seg(462), Conteudo::Texto),
            Decisao::Admitido,
            "e a janela de 60s a libera"
        );
    }

    // ─── diagnostico ────────────────────────────────────────────────────

    #[test]
    fn diagnostico_conta_a_hora_e_o_teto() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(50, seg(60));
        c.set_max(Some(7));
        for i in 0..5 {
            assert_eq!(
                c.admitir(CHAT, t0 + seg(i * 100), Conteudo::Texto),
                Decisao::Admitido
            );
        }
        let d = c.diagnosticar(t0 + seg(500));
        assert_eq!(d.turnos_ultima_hora, 5, "5 admitidos dentro da hora");
        assert_eq!(d.max_por_janela, 7, "knob do operador no relatorio");
        assert_eq!(d.janela_s, 60);
        assert_eq!(d.chats_ativos, 1);
        // Uma hora depois, a contagem esvazia sozinha.
        let d2 = c.diagnosticar(t0 + seg(4101));
        assert_eq!(d2.turnos_ultima_hora, 0);
    }
}
