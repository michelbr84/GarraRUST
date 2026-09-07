//! O que o turno **realmente** usou (#984).
//!
//! O `/stats` mostrava contadores globais e nada sobre o LLM. A issue pede
//! provider e modelo *efetivos*, e faz a distincao certa: o configurado nao e o
//! efetivo, porque o runtime resolve override, prefixo de modelo, `tools_model`
//! e fallback pelo caminho. Perguntar a config e perguntar a quem nao sabe.
//!
//! Entao quem responde e o runtime, gravando no fim de cada turno o que de fato
//! aconteceu. O modelo vem da **resposta do provider**, e nao do pedido: e o
//! unico valor que sobreviveu a todas as resolucoes.

use std::collections::{HashMap, VecDeque};

/// Quantas sessoes o registro guarda.
///
/// E um mapa em memoria alimentado por session_id vindo de request, entao sem
/// teto ele cresce com o numero de sessoes que ja passaram — inclusive as que o
/// gateway ja esqueceu. 512 cobre uso real de sobra e o custo por entrada e uma
/// dezena de bytes mais duas strings curtas.
const MAX_SESSOES: usize = 512;

/// O que um turno usou, do ponto de vista de quem executou.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnStats {
    /// O provider que serviu a resposta.
    pub provider: String,
    /// O melhor nome de modelo conhecido para o turno.
    ///
    /// Ver [`Self::model_confirmado`]: pode ser o que o provider **reportou**
    /// ou apenas o que foi **pedido**, e a diferenca importa.
    pub model: String,
    /// `true` quando `model` veio da resposta do provider.
    ///
    /// O caminho nao-streaming recebe uma `LlmResponse`, que traz o modelo que
    /// de fato respondeu — o unico valor que sobreviveu a override, prefixo,
    /// `tools_model` e fallback. O streaming recebe `StreamEvent`, que so tem
    /// deltas de texto e uso de ferramenta: ali o melhor que se sabe e o modelo
    /// pedido. Chamar os dois de "efetivo" seria mentir sobre metade dos
    /// turnos, entao o `/stats` diz qual dos dois esta mostrando.
    pub model_confirmado: bool,
    /// `true` quando o provider informou contagem de tokens.
    ///
    /// Streaming nao informa, e zero-porque-nao-sei e diferente de
    /// zero-porque-nao-usou.
    pub tokens_conhecidos: bool,
    /// `true` quando o primario falhou e um fallback serviu.
    pub fallback: bool,
    /// O modo em vigor no turno — deduzido, quando o escolhido foi `auto`.
    pub mode: Option<String>,
    /// Quantas ferramentas o turno chegou a executar.
    pub tool_calls: usize,
    /// Tokens somados de todas as chamadas do turno, quando o provider informa.
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Quanto o turno levou, ponta a ponta.
    pub latency_ms: u64,
}

/// Guarda o ultimo turno de cada sessao, com teto.
#[derive(Debug, Default)]
pub struct TurnStatsRegistry {
    por_sessao: HashMap<String, TurnStats>,
    /// Ordem de chegada, para saber quem sai quando o teto estoura.
    ordem: VecDeque<String>,
}

impl TurnStatsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra o turno de uma sessao, substituindo o anterior.
    pub fn record(&mut self, session_id: &str, stats: TurnStats) {
        if self
            .por_sessao
            .insert(session_id.to_string(), stats)
            .is_none()
        {
            self.ordem.push_back(session_id.to_string());
            while self.ordem.len() > MAX_SESSOES {
                if let Some(antiga) = self.ordem.pop_front() {
                    self.por_sessao.remove(&antiga);
                }
            }
        }
    }

    /// O ultimo turno de uma sessao, se houver.
    pub fn get(&self, session_id: &str) -> Option<&TurnStats> {
        self.por_sessao.get(session_id)
    }

    pub fn len(&self) -> usize {
        self.por_sessao.len()
    }

    pub fn is_empty(&self) -> bool {
        self.por_sessao.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(model: &str) -> TurnStats {
        TurnStats {
            provider: "ollama".into(),
            model: model.into(),
            ..TurnStats::default()
        }
    }

    #[test]
    fn guarda_o_ultimo_turno_de_cada_sessao() {
        let mut r = TurnStatsRegistry::new();
        r.record("a", stats("m1"));
        r.record("b", stats("m2"));
        r.record("a", stats("m3"));

        assert_eq!(r.get("a").map(|s| s.model.as_str()), Some("m3"));
        assert_eq!(r.get("b").map(|s| s.model.as_str()), Some("m2"));
        assert_eq!(r.get("nunca-vista"), None);
        assert_eq!(r.len(), 2, "regravar a mesma sessao nao duplica entrada");
    }

    /// O teto existe porque a chave vem de request.
    ///
    /// Sem ele, o mapa cresce com o numero de sessoes que ja passaram — e
    /// `X-Session-Id` e escolhido por quem chama, entao "quantas ja passaram"
    /// nao tem limite natural.
    #[test]
    fn o_teto_evita_crescimento_sem_limite() {
        let mut r = TurnStatsRegistry::new();
        for i in 0..(MAX_SESSOES + 50) {
            r.record(&format!("s{i}"), stats("m"));
        }

        assert_eq!(r.len(), MAX_SESSOES);
        assert_eq!(r.get("s0"), None, "as mais antigas saem");
        assert!(
            r.get(&format!("s{}", MAX_SESSOES + 49)).is_some(),
            "a mais recente fica"
        );
    }

    /// Regravar nao pode furar o teto pela porta dos fundos.
    ///
    /// Se `record` empurrasse na fila de ordem toda vez, uma sessao ativa
    /// sozinha expulsaria todas as outras e a fila cresceria sem limite mesmo
    /// com o mapa pequeno.
    #[test]
    fn regravar_nao_infla_a_fila_de_ordem() {
        let mut r = TurnStatsRegistry::new();
        r.record("viva", stats("m"));
        for _ in 0..1000 {
            r.record("viva", stats("m"));
        }
        r.record("outra", stats("m"));

        assert_eq!(r.len(), 2);
        assert!(
            r.get("viva").is_some(),
            "a sessao ativa nao se auto-expulsa"
        );
        assert!(r.get("outra").is_some());
    }
}
