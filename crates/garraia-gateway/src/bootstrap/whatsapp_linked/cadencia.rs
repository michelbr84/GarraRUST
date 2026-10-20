//! #1650: guarda de cadencia ENTRE turnos, por chat.
//!
//! O detector de loop do runtime so enxerga repeticao *dentro* de um turno.
//! Um DM que vira ping-pong com outro agente (auto-resposta) chega aqui como
//! mensagens separadas a cada poucos segundos, e cada uma e uma chamada paga
//! ao provider. Esta guarda corta o turno quando um chat passa de
//! `max_por_janela` turnos admitidos dentro de `janela`.
//!
//! Modulo puro: o instante entra por parametro (nada de relogio aqui), nada de
//! I/O, nada de identidade — a chave e o `chat_jid` e so ela. Estado em
//! memoria; um restart zera, de proposito (fail-safe, nao ledger).

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Teto padrao conservador: bem acima de uma conversa humana, bem abaixo do
/// incidente (um turno a cada 2-10s sustentado).
pub const MAX_TURNOS_POR_JANELA: usize = 20;
/// Janela deslizante do teto.
pub const JANELA: Duration = Duration::from_secs(60);
/// Teto de chats lembrados; passando disso, os ociosos sao descartados.
const CHATS_MAX: usize = 4096;

/// O que fazer com o turno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decisao {
    /// Dentro do teto: segue para o modelo.
    Admitido,
    /// Estourou o teto. `turnos` = quantos ja ocorreram na janela.
    /// `primeiro_corte` e `true` so no primeiro corte da rajada: e quando vale
    /// avisar uma unica vez, em vez de responder a cada mensagem cortada.
    Cortado { turnos: usize, primeiro_corte: bool },
}

#[derive(Debug)]
pub struct Cadencia {
    max: usize,
    janela: Duration,
    turnos: HashMap<String, VecDeque<Instant>>,
    /// Chats em corte desde o ultimo `Admitido` (para avisar uma vez so).
    em_corte: HashMap<String, ()>,
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
            janela,
            turnos: HashMap::new(),
            em_corte: HashMap::new(),
        }
    }

    /// Decide e registra. Turno cortado **nao** conta na janela: a tempestade
    /// nao se auto-prolonga, e quando o outro lado cala o chat volta sozinho.
    pub fn admitir(&mut self, chat: &str, agora: Instant) -> Decisao {
        if self.turnos.len() >= CHATS_MAX && !self.turnos.contains_key(chat) {
            self.varrer(agora);
        }
        let fila = self.turnos.entry(chat.to_string()).or_default();
        while fila
            .front()
            .is_some_and(|t| agora.saturating_duration_since(*t) >= self.janela)
        {
            fila.pop_front();
        }
        if fila.len() >= self.max {
            let turnos = fila.len();
            let primeiro_corte = self.em_corte.insert(chat.to_string(), ()).is_none();
            return Decisao::Cortado {
                turnos,
                primeiro_corte,
            };
        }
        fila.push_back(agora);
        self.em_corte.remove(chat);
        Decisao::Admitido
    }

    /// Turnos admitidos na janela corrente (para diagnostico).
    #[cfg(test)]
    pub fn turnos_na_janela(&self, chat: &str, agora: Instant) -> usize {
        self.turnos.get(chat).map_or(0, |f| {
            f.iter()
                .filter(|t| agora.saturating_duration_since(**t) < self.janela)
                .count()
        })
    }

    fn varrer(&mut self, agora: Instant) {
        let janela = self.janela;
        self.turnos.retain(|_, f| {
            f.back()
                .is_some_and(|t| agora.saturating_duration_since(*t) < janela)
        });
        let vivos = &self.turnos;
        self.em_corte.retain(|c, _| vivos.contains_key(c));
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
            assert_eq!(c.admitir(CHAT, t0 + seg(i * 30)), Decisao::Admitido);
        }
    }

    #[test]
    fn rajada_estoura_o_teto_e_avisa_so_uma_vez() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(3, seg(60));
        for i in 0..3 {
            assert_eq!(c.admitir(CHAT, t0 + seg(i * 2)), Decisao::Admitido);
        }
        assert_eq!(
            c.admitir(CHAT, t0 + seg(6)),
            Decisao::Cortado {
                turnos: 3,
                primeiro_corte: true
            }
        );
        assert_eq!(
            c.admitir(CHAT, t0 + seg(8)),
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
        assert_eq!(c.admitir(CHAT, t0), Decisao::Admitido);
        assert_eq!(c.admitir(CHAT, t0 + seg(1)), Decisao::Admitido);
        // Cortados durante a janela nao entram na contagem...
        for i in 2..30 {
            assert!(matches!(
                c.admitir(CHAT, t0 + seg(i)),
                Decisao::Cortado { .. }
            ));
        }
        // ...entao 60s depois do primeiro admitido o chat volta.
        assert_eq!(c.admitir(CHAT, t0 + seg(61)), Decisao::Admitido);
    }

    #[test]
    fn chats_sao_independentes() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(1, seg(60));
        assert_eq!(c.admitir("a", t0), Decisao::Admitido);
        assert_eq!(c.admitir("b", t0), Decisao::Admitido);
        assert!(matches!(c.admitir("a", t0), Decisao::Cortado { .. }));
    }

    #[test]
    fn novo_corte_depois_de_recuperar_avisa_de_novo() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(1, seg(10));
        c.admitir(CHAT, t0);
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(1)),
            Decisao::Cortado {
                primeiro_corte: true,
                ..
            }
        ));
        assert_eq!(c.admitir(CHAT, t0 + seg(11)), Decisao::Admitido);
        assert!(matches!(
            c.admitir(CHAT, t0 + seg(12)),
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
        assert_eq!(c.admitir(CHAT, t0), Decisao::Admitido);
    }

    #[test]
    fn contagem_da_janela() {
        let t0 = Instant::now();
        let mut c = Cadencia::new(5, seg(60));
        c.admitir(CHAT, t0);
        c.admitir(CHAT, t0 + seg(30));
        assert_eq!(c.turnos_na_janela(CHAT, t0 + seg(40)), 2);
        assert_eq!(c.turnos_na_janela(CHAT, t0 + seg(70)), 1);
    }
}
