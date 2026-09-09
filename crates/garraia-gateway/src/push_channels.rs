//! Os quatro canais *push* e o que o gateway consegue afirmar sobre eles.
//!
//! Canal *pull* (Telegram, Discord, Slack, IRC, Signal, Matrix, …) abre uma
//! conexao persistente no boot e entra no [`ChannelRegistry`]. Canal *push*
//! (WhatsApp, Google Chat, Teams, LINE) nao abre conexao nenhuma: o
//! `Vec<Arc<_>>` dele vira **estado da rota** `/webhooks/*`. Isso e desenho,
//! nao omissao — esta escrito em cada `bootstrap/<canal>.rs`.
//!
//! A consequencia e a #1079: o `GET /api/channels` derivava status do
//! registry, entao os quatro push davam `active == false` para sempre e,
//! por terem `needs_secret: true`, saiam classificados como `"offline"`
//! num gateway saudavel que estava recebendo webhook e respondendo.
//!
//! Este modulo e a segunda fonte que faltava. Ele guarda os mesmos
//! `Vec<Arc<_>>` que viram estado de rota, entao a resposta do `/api/channels`
//! sai do que **de fato subiu**, e nao do que estava escrito no arquivo de
//! config.
//!
//! ## Por que nao derivar do `AppConfig`
//!
//! A #1079 propos derivar de "ha pelo menos um configurado e habilitado no
//! `AppConfig`". Funciona para o sintoma, mas erra na direcao perigosa: um
//! canal push pode estar **configurado e ainda assim nao subir**. O
//! `build_line_channels` descarta canal com `channel_secret` invalido
//! (#1051), o `build_teams_channels` descarta canal sem `app_id`, o
//! `build_google_chat_channels` descarta canal sem `audience`, e desde a
//! #1070 o `WhatsAppChannel::new` recusa canal sem `app_secret`. Em todos
//! esses casos o config diz "configurado" e a rota tem zero canais.
//!
//! Status derivado de config mostraria **melhor** do que a realidade. A
//! #1079 mostra pior, o que ja destroi a confianca na tela; mostrar melhor
//! e o erro que faz um operador acreditar que o webhook esta protegido
//! quando o canal sequer existe. Entao o status vem do que subiu.
//!
//! [`ChannelRegistry`]: garraia_channels::ChannelRegistry

use garraia_channels::google_chat::webhook::GoogleChatState;
use garraia_channels::line_channel::webhook::LineState;
use garraia_channels::teams::webhook::TeamsState;
use garraia_channels::whatsapp::webhook::WhatsAppState;

/// Como um canal recebe mensagem. Coluna nova do `KNOWN_CHANNELS` (#1079).
///
/// E aqui que mora o conhecimento de "quem e push" — em **um** lugar. O
/// [`PushChannelStates::mounted`] e conferido contra esta coluna por um teste
/// (`todo_canal_push_da_tabela_e_conhecido_pelo_struct`), entao as duas
/// fontes nao conseguem divergir em silencio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    /// Abre conexao persistente no boot e entra no `ChannelRegistry`.
    Pull,
    /// Nao abre conexao: vira estado da rota `/webhooks/*`.
    Push,
}

/// Os `Vec<Arc<_>>` dos quatro canais push, juntos.
///
/// Substitui os quatro parametros posicionais que o `build_router` recebia.
/// Nos dois call-sites de teste eles eram quatro `Arc::new(Vec::new())`
/// seguidos, distinguidos so por comentario — trocar dois de lugar compilava
/// e montava o webhook do Teams com os canais do LINE.
#[derive(Clone)]
pub struct PushChannelStates {
    pub whatsapp: WhatsAppState,
    pub google_chat: GoogleChatState,
    pub teams: TeamsState,
    pub line: LineState,
}

impl PushChannelStates {
    /// Nenhum canal push. Para testes de rota que nao exercitam `/webhooks/*`.
    pub fn empty() -> Self {
        Self {
            whatsapp: std::sync::Arc::new(Vec::new()),
            google_chat: std::sync::Arc::new(Vec::new()),
            teams: std::sync::Arc::new(Vec::new()),
            line: std::sync::Arc::new(Vec::new()),
        }
    }

    /// Quantos canais deste `id` subiram de fato.
    ///
    /// `None` significa "este `id` nao e um canal push que eu conheco" — e
    /// nao "zero". A distincao importa: `Some(0)` e um canal push que existe
    /// no gateway e nao subiu (mostrar `"offline"` esta certo); `None` e a
    /// tabela `KNOWN_CHANNELS` e este struct discordando sobre quem e push,
    /// que e defeito de codigo e nao estado de runtime.
    pub fn mounted(&self, id: &str) -> Option<usize> {
        match id {
            "whatsapp" => Some(self.whatsapp.len()),
            "google_chat" => Some(self.google_chat.len()),
            "teams" => Some(self.teams.len()),
            "line" => Some(self.line.len()),
            _ => None,
        }
    }
}

impl std::fmt::Debug for PushChannelStates {
    /// So as contagens. Um canal push carrega segredo (`app_secret` da Meta,
    /// `channel_secret` do LINE, credencial de service account do Google
    /// Chat) e a regra 6 do `CLAUDE.md` proibe que isso apareca em log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushChannelStates")
            .field("whatsapp", &self.whatsapp.len())
            .field("google_chat", &self.google_chat.len())
            .field("teams", &self.teams.len())
            .field("line", &self.line.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vazio_conhece_os_quatro_push_e_reporta_zero() {
        let p = PushChannelStates::empty();
        for id in ["whatsapp", "google_chat", "teams", "line"] {
            assert_eq!(
                p.mounted(id),
                Some(0),
                "{id} e push: deve responder Some(0), nao None"
            );
        }
    }

    #[test]
    fn canal_pull_nao_e_conhecido_pelo_struct() {
        let p = PushChannelStates::empty();
        for id in ["telegram", "discord", "slack", "irc", "signal", "matrix"] {
            assert_eq!(
                p.mounted(id),
                None,
                "{id} e pull: o status dele vem do registry, nao daqui"
            );
        }
    }

    #[test]
    fn id_desconhecido_da_none_e_nao_zero() {
        // `Some(0)` diria "canal push que nao subiu". Para um id que nao
        // existe a resposta certa e "nao sei", para o chamador poder tratar
        // como defeito em vez de estampar "offline" numa linha inventada.
        assert_eq!(PushChannelStates::empty().mounted("nao-existe"), None);
    }

    #[test]
    fn debug_nao_vaza_conteudo_dos_canais() {
        let s = format!("{:?}", PushChannelStates::empty());
        assert!(s.contains("whatsapp"), "esperava as contagens: {s}");
        assert!(
            !s.contains("secret") && !s.contains("token"),
            "Debug nao pode carregar segredo: {s}"
        );
    }
}
