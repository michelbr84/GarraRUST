//! #1422: as mensagens que o WhatsApp pessoal **recusou**, por motivo — o que
//! a operadora ve no Web Console para entender por que alguem nao recebe
//! resposta, sem ler log e sem expor ninguem.
//!
//! Modulo puro: nada de relogio (quem registra passa o instante), nada de
//! I/O, nada de identidade inteira. Cada rejeicao guarda o **motivo**, os
//! quatro ultimos digitos (`…1234`, o mesmo mascaramento da politica) e se
//! veio de grupo. Retencao definida aqui, nao em prosa: contadores **desde o
//! boot** do gateway, os ultimos [`RECENTES_MAX`] com instante, e tudo zera
//! num restart ou por um reset explicito (auditado) da operadora.
//!
//! Dois consumidores com dois niveis de detalhe: o `/api/diagnostics`
//! (auth-free) recebe so as **contagens** ([`Resumo::so_contagens`]); a API
//! admin autenticada recebe o resumo inteiro, com os finais.

use std::collections::{BTreeMap, VecDeque};

use serde::Serialize;

/// Quantas rejeicoes recentes ficam na memoria (as mais novas primeiro).
pub const RECENTES_MAX: usize = 50;

/// Por que a mensagem nao chegou ao modelo. Lista fechada: a UI e a
/// remediacao de cada uma sao decididas por motivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Motivo {
    /// Admissao `restricted` e o remetente nao esta declarado (nem pareado).
    #[serde(rename = "restricted_policy")]
    Restrita,
    /// Remetente so com `@lid`, sem numero: um numero no `allow` nunca casa.
    #[serde(rename = "unresolved_lid")]
    LidSemNumero,
    /// `blocked: true` na politica — vence admissao aberta e pareamento.
    #[serde(rename = "blocked_user")]
    Bloqueado,
    /// `channels.whatsapp_linked.enabled` nao e `true` na config viva.
    #[serde(rename = "channel_disabled")]
    CanalDesligado,
    /// Admitido, mas o texto caiu no detector de injecao de prompt.
    #[serde(rename = "prompt_injection")]
    Injecao,
    /// Passou pelo portao e a politica por principal nao admitiu (call-site
    /// divergente do portao; nunca deveria acontecer, e por isso e contado).
    #[serde(rename = "policy_not_admitted")]
    Politica,
}

impl Motivo {
    /// Todos, na ordem em que o console os mostra.
    pub const TODOS: [Motivo; 6] = [
        Motivo::Restrita,
        Motivo::LidSemNumero,
        Motivo::Bloqueado,
        Motivo::CanalDesligado,
        Motivo::Injecao,
        Motivo::Politica,
    ];

    /// A grafia serializada (a mesma do JSON e do `data-reason` da UI).
    pub fn as_str(self) -> &'static str {
        match self {
            Motivo::Restrita => "restricted_policy",
            Motivo::LidSemNumero => "unresolved_lid",
            Motivo::Bloqueado => "blocked_user",
            Motivo::CanalDesligado => "channel_disabled",
            Motivo::Injecao => "prompt_injection",
            Motivo::Politica => "policy_not_admitted",
        }
    }

    /// O que a operadora pode fazer a respeito — uma acao por motivo, a
    /// mesma que o console liga ao botao da linha.
    pub fn remediacao(self) -> &'static str {
        match self {
            Motivo::Restrita => {
                "autorize o numero (nivel `read` por padrao) ou abra a admissao de proposito"
            }
            Motivo::LidSemNumero => {
                "gere um codigo com `/pair` e peca para a pessoa envia-lo, ou autorize o `<id>@lid` inteiro"
            }
            Motivo::Bloqueado => "desbloqueie a pessoa na politica de acesso, se foi engano",
            Motivo::CanalDesligado => "ligue `channels.whatsapp_linked.enabled: true`",
            Motivo::Injecao => "nada a fazer: o texto foi recusado, o remetente continua admitido",
            Motivo::Politica => "revise a politica de acesso; se persistir, abra uma issue",
        }
    }

    fn indice(self) -> usize {
        Motivo::TODOS
            .iter()
            .position(|m| *m == self)
            .unwrap_or_default()
    }
}

/// Uma rejeicao, como sai para a API admin. Nunca a identidade inteira.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rejeicao {
    /// UTC ISO 8601 com `Z`.
    pub at: String,
    pub reason: Motivo,
    /// `…1234` — os quatro ultimos digitos, com a reticencia da politica.
    pub identity_last4: String,
    pub group: bool,
}

/// A retencao, dita pela API para o console nao precisar adivinhar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Retencao {
    pub recent_max: usize,
    pub scope: &'static str,
    pub reset: &'static str,
}

/// O que a API admin devolve; [`Resumo::so_contagens`] e a versao auth-free.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resumo {
    pub total: u64,
    /// Motivo → contagem desde o boot (ou desde o ultimo reset).
    pub by_reason: BTreeMap<&'static str, u64>,
    /// Remediacao por motivo, para a UI nao carregar a tabela.
    pub remediation: BTreeMap<&'static str, &'static str>,
    /// As mais novas primeiro, ate [`RECENTES_MAX`].
    pub recent: Vec<Rejeicao>,
    /// Instante do ultimo reset explicito, se houve.
    pub last_reset: Option<String>,
    pub retention: Retencao,
}

impl Resumo {
    /// So contagens: o que o `/api/diagnostics` (sem autenticacao) pode dizer.
    pub fn so_contagens(&self) -> serde_json::Value {
        serde_json::json!({
            "total": self.total,
            "by_reason": self.by_reason,
        })
    }

    pub fn de(&self, motivo: Motivo) -> u64 {
        self.by_reason.get(motivo.as_str()).copied().unwrap_or(0)
    }
}

/// O estado que o runtime guarda atras de um `Mutex`.
#[derive(Debug, Default)]
pub struct Rejeicoes {
    totais: [u64; 6],
    recentes: VecDeque<Rejeicao>,
    ultimo_reset: Option<String>,
}

/// Reduz qualquer identidade ao `…1234` da politica — defesa em profundidade:
/// mesmo que um call-site passe o JID inteiro, so o final entra aqui.
pub fn so_final4(identidade: &str) -> String {
    let base = identidade
        .trim_start_matches('…')
        .split(['@', ':'])
        .next()
        .unwrap_or_default();
    let digitos: Vec<char> = base.chars().filter(|c| c.is_ascii_digit()).collect();
    let fim: String = digitos.iter().rev().take(4).rev().collect();
    if fim.is_empty() {
        "…????".to_string()
    } else {
        format!("…{fim}")
    }
}

impl Rejeicoes {
    /// Conta mais uma; devolve o total do motivo depois de contar.
    pub fn registrar(&mut self, motivo: Motivo, identidade: &str, grupo: bool, agora: &str) -> u64 {
        let i = motivo.indice();
        self.totais[i] = self.totais[i].saturating_add(1);
        self.recentes.push_front(Rejeicao {
            at: agora.to_string(),
            reason: motivo,
            identity_last4: so_final4(identidade),
            group: grupo,
        });
        self.recentes.truncate(RECENTES_MAX);
        self.totais[i]
    }

    pub fn total(&self, motivo: Motivo) -> u64 {
        self.totais[motivo.indice()]
    }

    /// Zera contadores e recentes; devolve quantas rejeicoes foram apagadas.
    pub fn zerar(&mut self, agora: &str) -> u64 {
        let apagadas = self.totais.iter().sum();
        self.totais = [0; 6];
        self.recentes.clear();
        self.ultimo_reset = Some(agora.to_string());
        apagadas
    }

    pub fn resumo(&self) -> Resumo {
        let by_reason = Motivo::TODOS
            .iter()
            .map(|m| (m.as_str(), self.total(*m)))
            .collect();
        let remediation = Motivo::TODOS
            .iter()
            .map(|m| (m.as_str(), m.remediacao()))
            .collect();
        Resumo {
            total: self.totais.iter().sum(),
            by_reason,
            remediation,
            recent: self.recentes.iter().cloned().collect(),
            last_reset: self.ultimo_reset.clone(),
            retention: Retencao {
                recent_max: RECENTES_MAX,
                scope: "since_boot",
                reset: "gateway restart, or POST /admin/api/whatsapp/access/rejections/reset (audited)",
            },
        }
    }
}

/// UTC agora, ISO 8601 com `Z` — o unico ponto com relogio, para o runtime.
pub fn agora() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "2026-09-26T12:00:00Z";

    #[test]
    fn conta_por_motivo_e_guarda_as_recentes_mais_novas_primeiro() {
        let mut r = Rejeicoes::default();
        assert_eq!(
            r.registrar(Motivo::Restrita, "5511999998888@s.whatsapp.net", false, T),
            1
        );
        assert_eq!(r.registrar(Motivo::Restrita, "5521955554444", false, T), 2);
        assert_eq!(r.registrar(Motivo::Bloqueado, "…4321", true, T), 1);
        let s = r.resumo();
        assert_eq!(s.total, 3);
        assert_eq!(s.de(Motivo::Restrita), 2);
        assert_eq!(s.de(Motivo::Bloqueado), 1);
        assert_eq!(
            s.de(Motivo::LidSemNumero),
            0,
            "todo motivo aparece, mesmo zerado"
        );
        assert_eq!(s.by_reason.len(), Motivo::TODOS.len());
        assert_eq!(s.recent[0].identity_last4, "…4321");
        assert!(s.recent[0].group);
        assert_eq!(s.recent[2].identity_last4, "…8888");
    }

    #[test]
    fn nunca_guarda_a_identidade_inteira() {
        let mut r = Rejeicoes::default();
        r.registrar(Motivo::Restrita, "5511999998888@s.whatsapp.net", false, T);
        r.registrar(Motivo::LidSemNumero, "87654321098765@lid", false, T);
        r.registrar(Motivo::Injecao, "sem-digito", false, T);
        let json = serde_json::to_string(&r.resumo()).expect("serializa");
        assert!(!json.contains("5511999998888"), "{json}");
        assert!(!json.contains("87654321098765"), "{json}");
        assert!(
            json.contains("…8888") && json.contains("…8765") && json.contains("…????"),
            "{json}"
        );
        assert!(json.contains("\"reason\":\"unresolved_lid\""), "{json}");
    }

    #[test]
    fn recentes_tem_teto_e_os_contadores_nao() {
        let mut r = Rejeicoes::default();
        for i in 0..(RECENTES_MAX as u64 + 10) {
            r.registrar(Motivo::Restrita, &format!("55119999{i:05}"), false, T);
        }
        let s = r.resumo();
        assert_eq!(s.recent.len(), RECENTES_MAX);
        assert_eq!(s.total, RECENTES_MAX as u64 + 10);
        assert_eq!(s.retention.recent_max, RECENTES_MAX);
        assert_eq!(s.retention.scope, "since_boot");
    }

    #[test]
    fn zerar_devolve_quantas_apagou_e_marca_o_reset() {
        let mut r = Rejeicoes::default();
        r.registrar(Motivo::Restrita, "…1111", false, T);
        r.registrar(Motivo::CanalDesligado, "…2222", false, T);
        assert_eq!(r.zerar("2026-09-26T13:00:00Z"), 2);
        let s = r.resumo();
        assert_eq!(s.total, 0);
        assert!(s.recent.is_empty());
        assert_eq!(s.last_reset.as_deref(), Some("2026-09-26T13:00:00Z"));
        assert_eq!(r.zerar("2026-09-26T13:01:00Z"), 0, "zerar vazio apaga zero");
    }

    #[test]
    fn so_contagens_nao_carrega_finais_nem_recentes() {
        let mut r = Rejeicoes::default();
        r.registrar(Motivo::Bloqueado, "5511977776666", false, T);
        let v = r.resumo().so_contagens();
        let json = v.to_string();
        assert_eq!(v["total"], 1);
        assert_eq!(v["by_reason"]["blocked_user"], 1);
        assert!(!json.contains("6666") && !json.contains("recent"), "{json}");
    }

    #[test]
    fn cada_motivo_tem_grafia_e_remediacao_proprias() {
        let mut vistos = std::collections::HashSet::new();
        for m in Motivo::TODOS {
            assert!(vistos.insert(m.as_str()), "grafia repetida: {}", m.as_str());
            assert!(!m.remediacao().is_empty());
            assert_eq!(
                serde_json::to_string(&m).expect("serializa"),
                format!("\"{}\"", m.as_str())
            );
        }
    }
}
