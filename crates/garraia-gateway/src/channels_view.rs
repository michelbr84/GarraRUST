//! O estado de cada canal, numa funcao so (#1347, fatia 2).
//!
//! Duas superficies respondem "este Garra esta conectado ao canal X?": o
//! `GET /api/channels` (o console, para o operador) e a tool `garra_status`
//! (o proprio agente, para o usuario). Ate a #1347 as duas liam fontes
//! diferentes — o console consultava o supervisor do `whatsapp_linked` e o
//! [`PushChannelStates`]; a tool so lia o `ChannelRegistry`, onde nem o
//! `whatsapp_linked` nem os quatro canais push jamais entram. O resultado foi
//! o relato da issue: conectado ao WhatsApp, o Garra respondia que nao tinha
//! acesso ao WhatsApp.
//!
//! Aqui mora a tabela [`KNOWN_CHANNELS`], a regra [`channel_status`] e o
//! montador [`channel_rows`], que as duas superficies chamam. Uma so funcao,
//! para que nao possam discordar sobre o mesmo canal.
//!
//! [`PushChannelStates`]: crate::push_channels::PushChannelStates

use crate::push_channels::{ChannelKind, PushMounted};
use crate::state::AppState;

/// Known channels — display metadata mirrors `KNOWN_PROVIDERS`. The `id`
/// column matches `ChannelRegistry` entries for pull channels; `needs_secret`
/// is purely informational (the Web Console renders an amber pill when true
/// but the actual secret value never crosses the API boundary).
///
/// A coluna `kind` entrou pela #1079 e e a **unica** fonte de "quem e push".
/// Canal push nao entra no `ChannelRegistry` — o `Vec<Arc<_>>` dele vira
/// estado da rota `/webhooks/*` —, entao derivar status do registry dava
/// `"offline"` eterno para os quatro. O status deles vem do
/// [`PushChannelStates`], e um teste confere que todo `Push` desta tabela e
/// conhecido la, para as duas fontes nao divergirem em silencio.
pub(crate) const KNOWN_CHANNELS: &[(&str, &str, bool, ChannelKind)] = &[
    ("web", "Web Chat", false, ChannelKind::Pull),
    ("api", "REST API", false, ChannelKind::Pull),
    ("telegram", "Telegram", true, ChannelKind::Pull),
    ("discord", "Discord", true, ChannelKind::Pull),
    ("slack", "Slack", true, ChannelKind::Pull),
    ("whatsapp", "WhatsApp", true, ChannelKind::Push),
    // #1238 (fatia D). `needs_secret: false` de proposito: a credencial deste
    // canal nao e um valor de config, e um blob cifrado em `<data_dir>/
    // whatsapp/default/session.enc`. O console nao tem campo para pedir, entao
    // a pilula ambar de "falta o segredo" seria mentira. Quando ha sessao e a
    // ponte nao esta de pe, quem transforma o `optional` em `offline` e o
    // `provisioned` — ver `channel_status`.
    (
        "whatsapp_linked",
        "WhatsApp (dispositivo vinculado)",
        false,
        ChannelKind::Pull,
    ),
    ("imessage", "iMessage", false, ChannelKind::Pull),
    ("google_chat", "Google Chat", true, ChannelKind::Push),
    ("teams", "Microsoft Teams", true, ChannelKind::Push),
    ("line", "LINE", true, ChannelKind::Push),
    ("irc", "IRC", false, ChannelKind::Pull),
    ("signal", "Signal", false, ChannelKind::Pull),
    ("matrix", "Matrix", true, ChannelKind::Pull),
    ("openclaw", "OpenClaw", false, ChannelKind::Pull),
    ("mcp", "MCP", false, ChannelKind::Pull),
    ("cli", "CLI", false, ChannelKind::Pull),
];

/// Decide o `status` de uma linha do `/api/channels`.
///
/// Extraida do handler para poder ser exercitada sem montar router, pool nem
/// runtime: e a regra que a #1079 errava, e um teste dela vale mais que um
/// teste do JSON inteiro.
///
/// `mounted` e o que o [`PushChannelStates`] respondeu para este `id`:
/// `None` para canal pull (a resposta vem de `live`), e para canal push
/// significa que a tabela e o struct discordam — tratado como `"unknown"`
/// em vez de virar `"offline"` numa linha que ninguem consegue explicar.
///
/// `provisioned` responde "o operador ligou este canal?" para os canais cuja
/// credencial **nao** e um valor de config e por isso nao cabe no
/// `needs_secret` estatico (#1238: o `whatsapp_linked` guarda a sessao cifrada
/// no data dir). `None` mantem a regra antiga, onde `needs_secret` responde
/// pelos dois. A distincao importa: um canal que ninguem ligou esta
/// `"optional"` — nao ha defeito —, mas um canal que alguem ligou e que nao
/// esta de pe esta `"offline"`, e um console que mostrasse `"optional"` nos
/// dois casos esconderia exatamente a falha que o operador precisa ver.
pub(crate) fn channel_status(
    kind: ChannelKind,
    needs_secret: bool,
    live: bool,
    mounted: Option<usize>,
    provisioned: Option<bool>,
) -> &'static str {
    let up = match kind {
        ChannelKind::Pull => live,
        ChannelKind::Push => match mounted {
            Some(n) => n > 0,
            // A tabela diz push e o struct nao conhece o id. Defeito de
            // codigo, nao estado de runtime — nao vale mentir "offline".
            None => return "unknown",
        },
    };

    if up {
        "active"
    } else if provisioned.unwrap_or(needs_secret) {
        "offline"
    } else {
        "optional"
    }
}

/// Uma linha da tabela, com o status ja resolvido. Secret-free: so os
/// campos estaticos da tabela e o status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChannelRow {
    pub id: &'static str,
    pub display_name: &'static str,
    pub status: &'static str,
    pub needs_secret: bool,
}

/// Todas as linhas de [`KNOWN_CHANNELS`], com o status de agora.
///
/// O `/api/channels` serve todas; o `garra_status` serve as que nao sao
/// `optional`. As duas chamam esta funcao, entao nao podem discordar sobre o
/// estado de um canal. `push` e [`PushChannelStates::contagens`]: a rota tira
/// na hora, a tool guarda a do boot (as listas push nao mudam depois dele).
///
/// [`PushChannelStates::contagens`]: crate::push_channels::PushChannelStates::contagens
pub(crate) async fn channel_rows(state: &AppState, push: PushMounted) -> Vec<ChannelRow> {
    let live: Vec<String> = state
        .channels
        .read()
        .await
        .list()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let mut rows: Vec<ChannelRow> = Vec::with_capacity(KNOWN_CHANNELS.len());
    for (id, display, needs_secret, kind) in KNOWN_CHANNELS {
        // Canal push nunca aparece em `live` — nao entra no registry por
        // desenho (#1079). Consultar `mounted` so quando `kind` diz push
        // mantem o registry como fonte unica para os pull.
        let mounted = match kind {
            ChannelKind::Push => push.mounted(id),
            ChannelKind::Pull => None,
        };
        // #1238: o `whatsapp_linked` e pull mas **nao** entra no
        // `ChannelRegistry` — o `Channel` trait pressupoe `connect()` sobre um
        // objeto mutavel, e aqui quem vive e um processo filho Node com loop de
        // reconexao proprio. E o mesmo desencontro que a #1079 custou nos
        // canais push, entao a correcao e a mesma: o status sai de quem
        // observa o canal de verdade — o supervisor —, e a leitura e a MESMA
        // que o `/api/diagnostics` faz (`whatsapp_linked_health`), para as duas
        // telas nao poderem discordar sobre o mesmo canal.
        let (live_aqui, provisioned) = if *id == crate::bootstrap::WHATSAPP_LINKED_CONFIG_KEY {
            let (saude, _) =
                crate::bootstrap::whatsapp_linked_health(&state.config, &state.whatsapp_linked);
            (saude.healthy(), Some(saude.provisioned()))
        } else {
            (live.iter().any(|name| name == *id), None)
        };
        let status = channel_status(*kind, *needs_secret, live_aqui, mounted, provisioned);
        if status == "unknown" {
            tracing::warn!(
                channel = id,
                "KNOWN_CHANNELS marca este canal como push mas PushChannelStates nao o conhece"
            );
        }
        rows.push(ChannelRow {
            id,
            display_name: display,
            status,
            needs_secret: *needs_secret,
        });
    }
    rows
}

/// O prefixo de `session_id` que cada canal usa, na ordem em que precisa ser
/// testado (`whatsapp-linked-` antes de `whatsapp-`).
///
/// Sao os `format!("<canal>-{..}")` de `bootstrap/<canal>.rs` e do
/// `whatsapp_linked::session_id`. O Telegram com `chat_session_manager` ligado
/// usa UUID e nao casa aqui: `session.channel` sai `null` nesse caso (ver
/// [`channel_of_session`]).
const PREFIXOS_DE_SESSAO: &[(&str, &str)] = &[
    ("whatsapp-linked-", "whatsapp_linked"),
    ("whatsapp-", "whatsapp"),
    ("telegram-", "telegram"),
    ("discord-", "discord"),
    ("slack-", "slack"),
    ("signal-", "signal"),
    ("matrix-", "matrix"),
    ("google-chat-", "google_chat"),
    ("teams-", "teams"),
    ("line-", "line"),
    ("imessage-", "imessage"),
    ("irc-", "irc"),
    ("openclaw-", "openclaw"),
    ("mcp-", "mcp"),
];

/// Os canais cujo remetente nao e, por construcao, o operador na propria
/// maquina: qualquer um que conheca o numero, o bot ou a sala manda
/// mensagem, e o portao do canal decide quem passa. `web`, `api`, `cli` e
/// `mcp` ficam de fora — sao a propria maquina do operador ou um cliente com
/// credencial dele.
const CANAIS_REMOTOS: &[&str] = &[
    "whatsapp_linked",
    "whatsapp",
    "telegram",
    "discord",
    "slack",
    "signal",
    "matrix",
    "google_chat",
    "teams",
    "line",
    "imessage",
    "irc",
    "openclaw",
];

/// O canal de uma sessao, pelo prefixo do `session_id` (o id da tabela
/// [`KNOWN_CHANNELS`]). `None` quando o id nao tem prefixo de canal (web
/// chat, API, Telegram com sessao resolvida por UUID).
pub(crate) fn channel_of_session(session_id: &str) -> Option<&'static str> {
    PREFIXOS_DE_SESSAO
        .iter()
        .find(|(prefixo, _)| session_id.starts_with(prefixo))
        .map(|(_, canal)| *canal)
}

/// O canal e remoto no sentido de [`CANAIS_REMOTOS`]?
pub(crate) fn is_remote_channel(id: &str) -> bool {
    CANAIS_REMOTOS.contains(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixo_de_sessao_resolve_o_canal() {
        assert_eq!(
            channel_of_session("whatsapp-linked-5511987654321@s.whatsapp.net"),
            Some("whatsapp_linked")
        );
        assert_eq!(
            channel_of_session("whatsapp-5511987654321"),
            Some("whatsapp")
        );
        assert_eq!(
            channel_of_session("google-chat-spaces/AAA"),
            Some("google_chat")
        );
        assert_eq!(
            channel_of_session("telegram--1001234567890"),
            Some("telegram")
        );
        assert_eq!(channel_of_session("mcp-1234"), Some("mcp"));
        assert_eq!(
            channel_of_session("550e8400-e29b-41d4-a716-446655440000"),
            None
        );
        assert_eq!(channel_of_session("sessao-teste"), None);
    }

    /// Todo canal que sai do prefixo existe na tabela — senao o
    /// `session.channel` do `garra_status` nomearia um canal que o
    /// `channels` do mesmo relatorio nunca lista.
    #[test]
    fn todo_canal_de_prefixo_e_de_remoto_esta_na_tabela() {
        let ids: Vec<&str> = KNOWN_CHANNELS.iter().map(|(id, _, _, _)| *id).collect();
        for (_, canal) in PREFIXOS_DE_SESSAO {
            assert!(ids.contains(canal), "{canal} fora do KNOWN_CHANNELS");
        }
        for canal in CANAIS_REMOTOS {
            assert!(ids.contains(canal), "{canal} fora do KNOWN_CHANNELS");
        }
    }

    #[test]
    fn so_os_canais_de_mensagem_sao_remotos() {
        for local in ["web", "api", "cli", "mcp"] {
            assert!(!is_remote_channel(local), "{local}");
        }
        for remoto in ["whatsapp_linked", "whatsapp", "telegram", "discord"] {
            assert!(is_remote_channel(remoto), "{remoto}");
        }
    }
}
