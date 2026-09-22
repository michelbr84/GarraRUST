//! Quem pode aprovar, no turno seguinte, um pedido de confirmacao (#1343).
//!
//! O runtime guarda o pedido pausado em memoria quando o turno chega com
//! `ExecContext::approval_scope` (ver
//! `garraia_agents::tools::pending_approval`). Este modulo e o unico lugar
//! do gateway que monta esse escopo, e ele existe por um motivo so: cada
//! ponto que chama o runtime com um humano do outro lado precisa decidir
//! **quem** e esse humano, e a decisao tem de ser do servidor.
//!
//! | Caminho | Canal | Remetente |
//! |---|---|---|
//! | `ws.rs` (chat web) | `web` | nonce da conexao WebSocket |
//! | `parrot_ws.rs` (desktop) | `parrot` | nonce da conexao WebSocket |
//! | `openai_api.rs` | `openai` | dono da allowlist + SHA-256 do `Authorization` |
//! | `mobile_chat.rs` | `mobile` | `sub` do JWT |
//! | `bootstrap/<canal>.rs` | nome do canal | id do usuario na plataforma |
//! | `bootstrap/whatsapp_linked.rs` | `whatsapp_linked` | remetente normalizado |
//!
//! Os caminhos sem humano verificavel (a2a, openclaw, `POST
//! /api/sessions/{id}/messages`, `rest_v1` com historico vazio, tarefa
//! agendada via `process_heartbeat`) ficam **sem** escopo, e ali a pausa
//! continua terminal. O teste `tests/approval_scope_coverage.rs` decide cada
//! chamada do runtime — escopada por aqui, ou presa por arquivo e `fn` na
//! lista de excecoes — e confere o remetente de cada `com_escopo` contra uma
//! tabela por arquivo.
//!
//! O remetente nunca e um valor que o cliente escolhe: nem `X-User-Id`, nem
//! o `session_id` de um `resume`. Remetente vazio devolve o contexto sem
//! escopo (`ApprovalScope::new` recusa campo vazio), e sem escopo a pausa e
//! terminal — o lado certo para errar.

use garraia_agents::ApprovalScope;
use garraia_agents::exec_context::ExecContext;

/// Canal do chat web (`/ws`).
pub const CANAL_WEB: &str = "web";
/// Canal do overlay do desktop (`/ws/parrot`).
pub const CANAL_PARROT: &str = "parrot";
/// Canal da API compativel com OpenAI (`/v1/chat/completions`).
pub const CANAL_OPENAI: &str = "openai";
/// Canal do app mobile (`POST /chat`).
pub const CANAL_MOBILE: &str = "mobile";

/// Devolve `exec` com o escopo de aprovacao deste turno.
///
/// `sessao` tem de ser o mesmo `session_id` que vai para o runtime: um
/// escopo de outra sessao nao aprova nada (o runtime confere).
pub fn com_escopo(
    mut exec: ExecContext,
    canal: &str,
    sessao: &str,
    remetente: &str,
) -> ExecContext {
    exec.approval_scope = ApprovalScope::new(canal, sessao, remetente);
    exec
}

/// Uma identidade nova para uma conexao WebSocket.
///
/// Sem login no chat web, a unica prova de que o "sim" vem de quem viu o
/// pedido e ser **a mesma conexao**. O `resume` sem token e aceito enquanto
/// a sessao esta em memoria (`ws.rs`), entao o `session_id` sozinho nao
/// prova nada; o nonce e gerado aqui, no servidor, e nunca sai para o
/// cliente. Reconectou? Pergunta de novo.
pub fn nonce_de_conexao() -> String {
    format!("conn-{}", uuid::Uuid::new_v4())
}

/// O remetente da API compativel com OpenAI.
///
/// A rota e auth-free por desenho e o dono vem da allowlist local — o mesmo
/// para todo chamador. Por isso o remetente tambem carrega o SHA-256 do
/// header `Authorization` que chegou: o bearer nao prova identidade (a rota
/// nao o confere), mas um segundo cliente, com a mesma `X-Session-Id` e
/// outra credencial, nao consegue aprovar o pedido do primeiro. Sem dono, sem
/// escopo: devolve `None` e a pausa e terminal.
///
/// O hash e dos BYTES do header, nao da string: um `Authorization` com
/// obs-text (byte >= 0x80, que `HeaderValue::to_str` recusa) continua sendo
/// a credencial dele, e nunca cai na identidade compartilhada de quem nao
/// mandou header nenhum (#1343 A9).
///
/// O hash inteiro so vive no mapa em memoria; ele nao vai para log (o
/// registro so loga ferramenta e canal).
pub fn remetente_openai(
    dono: Option<&str>,
    authorization: Option<&axum::http::HeaderValue>,
) -> Option<String> {
    use sha2::{Digest, Sha256};
    let dono = dono?.trim();
    if dono.is_empty() {
        return None;
    }
    let credencial = match authorization
        .map(|a| a.as_bytes().trim_ascii())
        .filter(|a| !a.is_empty())
    {
        Some(a) => hex::encode(Sha256::digest(a)),
        None => "sem-credencial".to_string(),
    };
    Some(format!("{dono}#{credencial}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn com_escopo_preenche_canal_sessao_e_remetente() {
        let exec = com_escopo(ExecContext::default(), "web", "s1", "conn-a");
        let escopo = exec.approval_scope.expect("escopo");
        assert_eq!(escopo.channel(), "web");
        assert_eq!(escopo.session_id(), "s1");
        assert_eq!(escopo.sender(), "conn-a");
    }

    /// Remetente vazio nao vira identidade compartilhada: fica sem escopo.
    #[test]
    fn remetente_vazio_fica_sem_escopo() {
        let exec = com_escopo(ExecContext::default(), "web", "s1", "  ");
        assert!(exec.approval_scope.is_none());
    }

    /// O escopo nao apaga o que o contexto ja tinha (modo, diretorio).
    #[test]
    fn com_escopo_preserva_o_resto_do_contexto() {
        let exec = com_escopo(
            ExecContext::with_mode(Some("search".into())),
            "web",
            "s1",
            "a",
        );
        assert_eq!(exec.agent_mode.as_deref(), Some("search"));
    }

    #[test]
    fn nonce_de_conexao_nunca_repete() {
        assert_ne!(nonce_de_conexao(), nonce_de_conexao());
    }

    fn header(v: &[u8]) -> axum::http::HeaderValue {
        axum::http::HeaderValue::from_bytes(v).expect("header valido")
    }

    #[test]
    fn remetente_openai_sem_dono_e_none() {
        let h = header(b"Bearer x");
        assert_eq!(remetente_openai(None, Some(&h)), None);
        assert_eq!(remetente_openai(Some(" "), Some(&h)), None);
    }

    /// Outra credencial, outro remetente — e o valor cru do bearer nunca
    /// aparece no remetente.
    #[test]
    fn remetente_openai_separa_credenciais_e_nao_guarda_o_bearer() {
        let a = remetente_openai(Some("dono"), Some(&header(b"Bearer segredo-a"))).expect("a");
        let b = remetente_openai(Some("dono"), Some(&header(b"Bearer segredo-b"))).expect("b");
        let anon = remetente_openai(Some("dono"), None).expect("anon");
        assert_ne!(a, b);
        assert_ne!(a, anon);
        assert!(!a.contains("segredo-a"));
        assert_eq!(
            remetente_openai(Some("dono"), Some(&header(b"Bearer segredo-a"))),
            Some(a)
        );
        // Header so de espaco e o mesmo que nenhum.
        assert_eq!(
            remetente_openai(Some("dono"), Some(&header(b"   "))),
            Some(anon)
        );
    }

    /// #1343 A9: `Authorization` com bytes que nao sao UTF-8 (obs-text) e
    /// uma credencial como outra qualquer — nunca a identidade anonima, que
    /// e compartilhada por todo cliente sem header.
    #[test]
    fn authorization_nao_utf8_nao_vira_o_anonimo() {
        let obs_a = header(b"Bearer \xff\xfea");
        let obs_b = header(b"Bearer \xff\xfeb");
        assert!(obs_a.to_str().is_err(), "o caso e justamente o nao-UTF-8");
        let anon = remetente_openai(Some("dono"), None).expect("anon");
        let a = remetente_openai(Some("dono"), Some(&obs_a)).expect("a");
        let b = remetente_openai(Some("dono"), Some(&obs_b)).expect("b");
        assert_ne!(a, anon);
        assert_ne!(b, anon);
        assert_ne!(a, b);
    }
}
