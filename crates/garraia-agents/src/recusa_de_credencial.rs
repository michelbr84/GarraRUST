//! A ultima recusa de credencial (401/403) que cada provider devolveu (#1743).
//!
//! O `/api/health` sonda o provider sem credencial: a rota `GET /models` do
//! OpenRouter e publica e responde 200 com a chave morta, e a Anthropic nem e
//! sondada. Resultado: o check ficava verde enquanto TODO turno ao primario
//! voltava `status=401` e caia no fallback local (#1735/#1742) — o operador so
//! descobria a chave morta quando a qualidade das respostas caia.
//!
//! Quem sabe que a chave morreu e o runtime, no turno. Ele anota aqui, e o
//! health le, sem chamada extra ao provider. A anotacao expira quando um turno
//! do mesmo provider volta a passar — a chave foi trocada, ou era um 401
//! transitorio do lado de la.
//!
//! Guarda so o que o health precisa dizer: provider, status HTTP e quando. O
//! corpo da recusa fica de fora de proposito: `/api/health` e auth-free e o
//! corpo de um 401 pode carregar o prefixo da propria chave.

use std::collections::HashMap;
use std::time::SystemTime;

/// O que o runtime observou da credencial de um provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecusaDeCredencial {
    /// O id do provider — a chave da entrada `llm:` da config, que e o
    /// mesmo nome que o `/api/health` da ao check.
    pub provider: String,
    /// O status que o provider respondeu: 401 ou 403.
    pub status: u16,
    /// Quando a ultima recusa foi observada.
    pub quando: SystemTime,
    /// Quantas recusas seguidas desde o ultimo turno que passou.
    pub seguidas: u32,
}

/// As recusas em vigor, uma por provider.
#[derive(Debug, Default)]
pub struct RecusasDeCredencial {
    por_provider: HashMap<String, RecusaDeCredencial>,
}

impl RecusasDeCredencial {
    pub fn new() -> Self {
        Self::default()
    }

    /// Anota uma recusa. A seguinte do mesmo provider so atualiza status, hora
    /// e contador — e UMA linha por provider, nao um historico.
    pub fn registrar(&mut self, provider: &str, status: u16, quando: SystemTime) {
        match self.por_provider.get_mut(provider) {
            Some(recusa) => {
                recusa.status = status;
                recusa.quando = quando;
                recusa.seguidas = recusa.seguidas.saturating_add(1);
            }
            None => {
                self.por_provider.insert(
                    provider.to_string(),
                    RecusaDeCredencial {
                        provider: provider.to_string(),
                        status,
                        quando,
                        seguidas: 1,
                    },
                );
            }
        }
    }

    /// Um turno deste provider passou: a recusa expira. Devolve se havia uma.
    pub fn limpar(&mut self, provider: &str) -> bool {
        self.por_provider.remove(provider).is_some()
    }

    /// A recusa em vigor para um provider, se houver.
    pub fn get(&self, provider: &str) -> Option<&RecusaDeCredencial> {
        self.por_provider.get(provider)
    }

    /// Todas as recusas em vigor, em ordem de provider — deterministica para
    /// quem renderiza.
    pub fn todas(&self) -> Vec<RecusaDeCredencial> {
        let mut todas: Vec<RecusaDeCredencial> = self.por_provider.values().cloned().collect();
        todas.sort_by(|a, b| a.provider.cmp(&b.provider));
        todas
    }

    pub fn is_empty(&self) -> bool {
        self.por_provider.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn t(segundos: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(segundos)
    }

    #[test]
    fn registra_uma_linha_por_provider_e_conta_as_seguidas() {
        let mut r = RecusasDeCredencial::new();
        r.registrar("openrouter", 401, t(10));
        r.registrar("openrouter", 403, t(20));

        let recusa = r.get("openrouter").expect("anotada");
        assert_eq!(recusa.status, 403, "a ultima recusa e a que vale");
        assert_eq!(recusa.quando, t(20));
        assert_eq!(recusa.seguidas, 2);
        assert_eq!(r.todas().len(), 1);
    }

    #[test]
    fn turno_que_passa_expira_a_recusa_so_daquele_provider() {
        let mut r = RecusasDeCredencial::new();
        r.registrar("openrouter", 401, t(10));
        r.registrar("anthropic", 401, t(11));

        assert!(r.limpar("openrouter"));
        assert!(!r.limpar("openrouter"), "ja nao havia");
        assert!(r.get("openrouter").is_none());
        assert!(
            r.get("anthropic").is_some(),
            "o outro provider segue recusado"
        );
        assert!(!r.is_empty());
    }

    #[test]
    fn recusa_depois_de_expirar_recomeca_a_contagem() {
        let mut r = RecusasDeCredencial::new();
        r.registrar("openrouter", 401, t(10));
        r.registrar("openrouter", 401, t(11));
        r.limpar("openrouter");
        r.registrar("openrouter", 401, t(12));

        assert_eq!(r.get("openrouter").map(|x| x.seguidas), Some(1));
    }

    #[test]
    fn todas_sai_em_ordem_de_provider() {
        let mut r = RecusasDeCredencial::new();
        r.registrar("zeta", 401, t(1));
        r.registrar("alpha", 403, t(2));

        let todas = r.todas();
        let nomes: Vec<&str> = todas.iter().map(|x| x.provider.as_str()).collect();
        assert_eq!(nomes, vec!["alpha", "zeta"]);
    }
}
