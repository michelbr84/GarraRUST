//! #1409 / #1415: o que o admin ve de cada sessao alem do id — modo
//! escolhido, projeto ativo (nome, nunca caminho) e, no WhatsApp pessoal,
//! o **principal** e o **modo efetivo** do turno, calculados pelo mesmo
//! motor do `turno` (`perfil_do_turno` + `modo_do_piso`, ADR 0024/0025).
//! Nao ha segundo modelo: `principal_do_turno` sobre a config VIVA e a
//! identidade que a sessao guardou.

use serde_json::{Value, json};

use crate::bootstrap::whatsapp_linked_politica::{Principal, principal_do_turno};
use crate::bootstrap::{
    WHATSAPP_LINKED_CONFIG_KEY, whatsapp_linked_modo_do_piso, whatsapp_linked_perfil_do_turno,
    whatsapp_linked_settings,
};
use crate::state::{AppState, SessionState};

/// O snapshot de uma sessao, tirado com o lock do `DashMap` e solto antes
/// de qualquer `await`.
#[derive(Debug, Clone)]
pub struct ResumoDaSessao {
    pub id: String,
    pub tenant_id: String,
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
    pub connected: bool,
    pub history_len: usize,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub has_workspace: bool,
}

impl ResumoDaSessao {
    pub fn de(s: &SessionState) -> Self {
        Self {
            id: s.id.clone(),
            tenant_id: s.tenant_id.clone(),
            user_id: s.user_id.clone(),
            channel_id: s.channel_id.clone(),
            connected: s.connected,
            history_len: s.history.len(),
            project_id: s.project_id.clone(),
            project_name: s.project_name.clone(),
            has_workspace: s.working_dir.is_some(),
        }
    }

    /// E uma sessao do WhatsApp pessoal? (prefixo de `session_id` do canal)
    pub fn e_whatsapp_linked(&self) -> bool {
        self.id.starts_with("whatsapp-linked-")
            || self.channel_id.as_deref() == Some(WHATSAPP_LINKED_CONFIG_KEY)
    }

    /// Conversa de grupo? (JID de grupo termina em `@g.us`)
    pub fn e_grupo(&self) -> bool {
        self.id.ends_with("@g.us")
    }

    /// O JID da conversa (o que vem depois do prefixo).
    pub fn chat(&self) -> &str {
        self.id.strip_prefix("whatsapp-linked-").unwrap_or(&self.id)
    }
}

/// O principal e o modo efetivo de uma sessao do WhatsApp pessoal, pela
/// config viva. `None` para outras superficies.
pub fn principal_e_piso(state: &AppState, r: &ResumoDaSessao) -> Option<(Principal, String)> {
    if !r.e_whatsapp_linked() {
        return None;
    }
    let remetente = r.user_id.as_deref()?;
    let settings = whatsapp_linked_settings(&state.current_config());
    let principal = principal_do_turno(&settings, remetente, r.chat(), r.e_grupo(), false);
    let perfil = crate::bootstrap::politica_de_execucao(&state.config).perfil;
    let piso = whatsapp_linked_modo_do_piso(
        whatsapp_linked_perfil_do_turno(perfil, &settings, remetente, r.e_grupo()),
        &settings,
    );
    Some((principal, piso))
}

/// A linha da sessao para `GET /admin/api/sessions` (e o cabecalho do
/// painel de capacidades). Sem caminho de workspace: so `has_workspace`.
pub async fn sessao_em_json(state: &AppState, r: ResumoDaSessao) -> Value {
    let chosen_mode = state.chosen_agent_mode_for(&r.id).await;
    let (principal, level, write, effective_mode) = match principal_e_piso(state, &r) {
        Some((p, piso)) => {
            let alcance = p.alcance();
            // O efetivo e o que o turno usa: a escolha da sessao (se houver e
            // se o principal a puder ter) vence o piso; sem escolha, o piso.
            let efetivo = chosen_mode.clone().unwrap_or(piso);
            (
                Some(p.as_str().to_string()),
                alcance.map(|a| a.nivel.as_str().to_string()),
                alcance.map(|a| a.write),
                Some(efetivo),
            )
        }
        None => (None, None, None, chosen_mode.clone()),
    };
    json!({
        "id": r.id,
        "tenant_id": r.tenant_id,
        "user_id": r.user_id,
        "channel_id": r.channel_id,
        "connected": r.connected,
        "history_len": r.history_len,
        "chosen_mode": chosen_mode,
        "effective_mode": effective_mode,
        "principal": principal,
        "level": level,
        "write": write,
        "project_id": r.project_id,
        "project_name": r.project_name,
        "has_workspace": r.has_workspace,
    })
}
