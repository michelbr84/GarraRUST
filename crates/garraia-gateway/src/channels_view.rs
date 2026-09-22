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
//! para que nao possam discordar sobre o mesmo canal: os fatos (vivo,
//! montado, ligado na config) sao lidos uma vez so, e cada linha sai com o
//! status do console e o do relatorio do agente lado a lado. Os dois so
//! divergem onde o console guarda a regra antiga de deixar o `needs_secret`
//! responder por "o operador ligou" ([`ChannelRow::status_do_agente`]).
//!
//! Aqui mora tambem a resposta a "quem fala nesta sessao?"
//! ([`sessao_do_operador`]), que decide o que o `garra_status` retem.
//!
//! [`PushChannelStates`]: crate::push_channels::PushChannelStates

use garraia_config::{AppConfig, GatewayConfig};

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

/// As linhas de [`KNOWN_CHANNELS`] que nao sao canal de mensagens, e por
/// isso nunca entram no relatorio do agente (#1347, C4).
///
/// `web` e `api` sao servidos pelo listener HTTP do proprio gateway e nunca
/// passam pelo `ChannelRegistry`; `cli` e `mcp` sao programas a parte
/// (`garraia chat`, `garraia mcp-server`), que este processo nao enxerga. No
/// `/api/channels` os quatro saem `optional` desde sempre. No relatorio, uma
/// linha deles diria "nao ligado" sobre a superficie em que o usuario esta
/// conversando — entao ficam de fora, e a nota do runtime
/// (`NOTA_GARRA_STATUS_*`) diz, com estes mesmos ids, que a lista nao fala
/// deles e que o canal da conversa esta em `session.channel`. Um teste do
/// `garra_status` confere que a nota cita cada id daqui.
pub(crate) const FORA_DO_RELATORIO_DO_AGENTE: &[&str] = &["web", "api", "cli", "mcp"];

/// O operador ligou o canal `id` na config? (#1347, C2)
///
/// E o mesmo filtro que cada `build_<canal>_channels` aplica antes de
/// construir o canal — uma secao de `channels` com `type` igual ao `id` e
/// sem `enabled: false`. Um token so no env nao liga nada: ele so preenche
/// a credencial de uma secao que existe (ver `bootstrap::telegram`).
fn ligado_na_config(config: &AppConfig, id: &str) -> bool {
    config
        .channels
        .values()
        .any(|c| c.channel_type == id && c.enabled != Some(false))
}

/// Uma linha da tabela, com o status ja resolvido. Secret-free: so os
/// campos estaticos da tabela e o status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChannelRow {
    pub id: &'static str,
    pub display_name: &'static str,
    /// O status do `GET /api/channels` — contrato do console, inalterado.
    pub status: &'static str,
    pub needs_secret: bool,
    /// O status que o relatorio do agente (`garra_status`) mostra, ou `None`
    /// quando o canal nao entra nele (#1347, C2 e C4).
    ///
    /// Os mesmos fatos do `status`, com uma diferenca: aqui quem responde
    /// "o operador ligou?" e a config ([`ligado_na_config`]), e nao o
    /// `needs_secret`. No console, um Telegram que ninguem configurou sai
    /// `offline` (a pilula de "falta o segredo"); no relatorio, sair
    /// `offline` diria ao modelo que o canal esta configurado e caido, e o
    /// modelo repetia isso ao usuario de uma instalacao nova. Canal que
    /// ninguem ligou fica `optional` e sai do relatorio; ligado e fora do ar
    /// continua `offline` — inclusive os que nao precisam de segredo (IRC,
    /// Signal), que o console chama de `optional`.
    pub status_do_agente: Option<&'static str>,
}

/// Todas as linhas de [`KNOWN_CHANNELS`], com o status de agora.
///
/// O `/api/channels` serve todas com `status`; o `garra_status` serve as que
/// tem [`ChannelRow::status_do_agente`]. As duas chamam esta funcao, entao
/// leem os mesmos fatos sobre cada canal. `push` e
/// [`PushChannelStates::contagens`]: a rota tira na hora, a tool guarda a do
/// boot (as listas push nao mudam depois dele).
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
        // O `whatsapp_linked` ja chega com o `provisioned` do disco; os
        // demais, com o da config.
        let ligado = provisioned.unwrap_or_else(|| ligado_na_config(&state.config, id));
        let status_do_agente = if FORA_DO_RELATORIO_DO_AGENTE.contains(id) {
            None
        } else {
            match channel_status(*kind, *needs_secret, live_aqui, mounted, Some(ligado)) {
                "optional" => None,
                outro => Some(outro),
            }
        };
        rows.push(ChannelRow {
            id,
            display_name: display,
            status,
            needs_secret: *needs_secret,
            status_do_agente,
        });
    }
    rows
}

/// O prefixo de `session_id` que cada superficie usa, na ordem em que
/// precisa ser testado (`whatsapp-linked-` antes de `whatsapp-`).
///
/// Sao os `format!("<canal>-{..}")` de `bootstrap/<canal>.rs`, o
/// `whatsapp_linked::session_id`, o `mobile_session_id` e o `a2a:{task}`. O
/// Telegram com `chat_session_manager` ligado usa UUID e nao casa aqui — e
/// por isso o prefixo **nunca** decide sozinho quem fala numa sessao (ver
/// [`sessao_do_operador`]): ele so serve de sinal a mais de canal remoto e de
/// rotulo de `session.channel` quando nenhum turno gravou superficie.
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
    ("mobile-", "mobile"),
    ("a2a:", "a2a"),
];

/// Quem pode estar do outro lado de uma superficie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Remetente {
    /// O operador na propria maquina, ou um cliente com a credencial dele.
    Operador,
    /// Qualquer um que o portao do canal, uma conta aberta ou outro sistema
    /// deixou passar.
    Qualquer,
}

/// Cada nome de superficie que um ponto de entrada do gateway grava num
/// turno (o `channel_id` de `hydrate_session_history` / `persist_turn`, e as
/// fontes de `chat_session_keys`), com quem esta do outro lado (#1347, C1).
///
/// A lista e explicita de proposito, e o default e o lado seguro: nome fora
/// dela conta como [`Remetente::Qualquer`] ([`remetente_da_superficie`]).
const SUPERFICIES: &[(&str, Remetente)] = &[
    // `ws.rs`, o web chat deste gateway (`GET /` + `/ws`). O handshake passa
    // pelo anti-CSRF (#1182) e pelo gate de `gateway.api_key` (#1240); sem
    // chave, o boot so sobe em loopback (#1261). Ver
    // [`porta_do_operador_fechada`] para o opt-out.
    ("web", Remetente::Operador),
    // `api.rs` (`POST /api/chat` e o resto do Web Console): sob o gate de
    // `/api/*` (#1045). Mesma porta, mesma ressalva do opt-out.
    ("api", Remetente::Operador),
    // `openai_api.rs` (`/v1/chat/completions`), que grava `"vscode"` em todo
    // turno: rota de `gateway_auth::ROTAS_DE_CONVERSA` (#1240).
    ("vscode", Remetente::Operador),
    // `parrot_ws.rs`, o overlay do Garra Desktop: so origem Tauri, e o mesmo
    // gate de `api_key` no handshake.
    ("desktop", Remetente::Operador),
    // `garraia chat`, `garraia ask` (`ask-<uuid>`) e `garraia mcp-server`
    // (`mcp-<uuid>`): processos da propria CLI, falando por stdio com quem
    // os abriu. Hoje nenhum chega aqui — montam runtime proprio, que nao
    // registra `garra_status` nem grava superficie neste `AppState`, e uma
    // sessao sem superficie gravada e restrita. Ficam na tabela para a
    // classificacao nao depender de ninguem lembrar disso.
    ("cli", Remetente::Operador),
    ("mcp", Remetente::Operador),
    // `mobile_chat.rs` (`POST /chat`): JWT de uma conta que QUALQUER um cria
    // em `POST /auth/register`, rota aberta e fora do gate de `api_key`. Ter
    // conta neste Garra nao e ser o operador dele.
    ("mobile", Remetente::Qualquer),
    // `a2a.rs` (`/a2a/tasks`): outro agente, repassando conteudo de terceiros.
    ("a2a", Remetente::Qualquer),
    // Pontes e canais de mensagens: qualquer um que conheca o numero, o bot
    // ou a sala manda mensagem, e o portao do canal decide quem passa.
    ("openclaw", Remetente::Qualquer),
    ("telegram", Remetente::Qualquer),
    ("discord", Remetente::Qualquer),
    ("slack", Remetente::Qualquer),
    ("whatsapp", Remetente::Qualquer),
    ("whatsapp_linked", Remetente::Qualquer),
    ("imessage", Remetente::Qualquer),
    ("google_chat", Remetente::Qualquer),
    ("teams", Remetente::Qualquer),
    ("line", Remetente::Qualquer),
    ("irc", Remetente::Qualquer),
    ("signal", Remetente::Qualquer),
    ("matrix", Remetente::Qualquer),
];

/// Quem esta do outro lado da superficie `nome`. Nome desconhecido e
/// [`Remetente::Qualquer`]: so o que foi provado local ve o que e do
/// operador.
pub(crate) fn remetente_da_superficie(nome: &str) -> Remetente {
    SUPERFICIES
        .iter()
        .find(|(id, _)| *id == nome)
        .map(|(_, r)| *r)
        .unwrap_or(Remetente::Qualquer)
}

/// As superficies do operador so provam o operador enquanto a porta dele
/// esta fechada.
///
/// O boot (#1261, `server::refuse_exposed_bind`) so sobe em tres casos:
/// loopback, exposto com `gateway.api_key`, ou exposto pelo opt-out
/// `allow_unauthenticated_network_bind` sem chave. No terceiro, o web chat e
/// o `/api/*` respondem a quem alcanca a porta, e "e o web chat" deixa de
/// dizer "e o operador". Sem resolver o host aqui (DNS a cada chamada da
/// tool), a regra fica: chave configurada, ou opt-out desligado (que obriga
/// o loopback). Opt-out ligado sem chave conta como porta aberta, mesmo que
/// o bind seja loopback — erra para o lado seguro.
fn porta_do_operador_fechada(gateway: &GatewayConfig) -> bool {
    gateway.api_key_configurada() || !gateway.allow_unauthenticated_network_bind
}

/// O canal de uma sessao pelo prefixo do `session_id`. `None` quando o id
/// nao tem prefixo conhecido (web chat, API, Telegram por UUID).
pub(crate) fn channel_of_session(session_id: &str) -> Option<&'static str> {
    PREFIXOS_DE_SESSAO
        .iter()
        .find(|(prefixo, _)| session_id.starts_with(prefixo))
        .map(|(_, canal)| *canal)
}

/// O rotulo de `session.channel` no relatorio: a ultima superficie que um
/// turno gravou nesta sessao, ou o canal do prefixo quando nenhum gravou.
/// So rotulo — quem decide o que o relatorio retem e [`sessao_do_operador`].
pub(crate) fn rotulo_da_sessao(state: &AppState, session_id: &str) -> Option<String> {
    let gravado = state.sessions.get(session_id).and_then(|s| {
        s.channel_id
            .clone()
            .filter(|canal| s.canais_dos_turnos.contains(canal))
    });
    gravado.or_else(|| channel_of_session(session_id).map(str::to_string))
}

/// Esta sessao so pode ter o operador do outro lado? (#1347, C1)
///
/// O `garra_status` entrega o que e do operador (`working_dir`, provedores,
/// servidores MCP, versao exata) so quando isto e `true`. A resposta **nao**
/// sai do prefixo do `session_id`: a sessao do Telegram resolvida pelo
/// `chat_session_manager` e um UUID, sem prefixo, e a versao anterior desta
/// regra a tratava como local. A fonte e o que os pontos de entrada gravam:
///
/// 1. as superficies de todo turno desta sessao
///    (`SessionState::canais_dos_turnos`, gravado por
///    `hydrate_session_history` antes do turno) — todas precisam ser
///    [`Remetente::Operador`], e sem nenhuma gravada a sessao e desconhecida;
/// 2. as fontes de `chat_session_keys` da sessao — a marca persistente do
///    Telegram por UUID e do Chat Sync, que sobrevive a um restart e a um
///    turno local posterior numa sessao compartilhada;
/// 3. o prefixo do id, como sinal a mais de canal remoto;
/// 4. a porta do operador fechada ([`porta_do_operador_fechada`]).
///
/// Qualquer sinal remoto, qualquer falha de leitura ou nenhuma superficie
/// gravada responde `false`: fail-closed.
pub(crate) async fn sessao_do_operador(state: &AppState, session_id: &str) -> bool {
    if !porta_do_operador_fechada(&state.config.gateway) {
        return false;
    }
    if channel_of_session(session_id)
        .is_some_and(|canal| remetente_da_superficie(canal) == Remetente::Qualquer)
    {
        return false;
    }
    let superficies: Vec<String> = match state.sessions.get(session_id) {
        Some(s) => s.canais_dos_turnos.iter().cloned().collect(),
        None => return false,
    };
    if superficies.is_empty()
        || superficies
            .iter()
            .any(|s| remetente_da_superficie(s) != Remetente::Operador)
    {
        return false;
    }
    // Sem banco nao ha `chat_session_keys` — e tambem nao ha sessao por
    // UUID: o Telegram cai em `telegram-{chat_id}`
    // (`AppState::telegram_session_id`), e o prefixo acima ja o pegou.
    if let Some(store) = &state.session_store {
        let fontes = store.lock().await.get_sources_for_session(session_id);
        match fontes {
            Ok(fontes) => {
                if fontes
                    .iter()
                    .any(|f| remetente_da_superficie(f) != Remetente::Operador)
                {
                    return false;
                }
            }
            Err(e) => {
                tracing::warn!(
                    erro = %e,
                    "garra_status: chat_session_keys ilegivel; sessao tratada como remota"
                );
                return false;
            }
        }
    }
    true
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
        assert_eq!(channel_of_session("mobile-user-1"), Some("mobile"));
        assert_eq!(channel_of_session("a2a:tarefa-1"), Some("a2a"));
        // `mcp-<uuid>` e da CLI, nunca deste gateway: nao e prefixo daqui.
        assert_eq!(channel_of_session("mcp-1234"), None);
        assert_eq!(
            channel_of_session("550e8400-e29b-41d4-a716-446655440000"),
            None
        );
        assert_eq!(channel_of_session("sessao-teste"), None);
    }

    /// Todo canal da tabela do console tem uma classificacao escrita, e todo
    /// prefixo aponta para uma superficie de [`Remetente::Qualquer`] — o
    /// prefixo so pode tirar o operador de uma sessao, nunca por nela.
    #[test]
    fn toda_superficie_conhecida_esta_classificada() {
        let classificadas: Vec<&str> = SUPERFICIES.iter().map(|(id, _)| *id).collect();
        for (id, _, _, _) in KNOWN_CHANNELS {
            assert!(classificadas.contains(id), "{id} sem classificacao");
        }
        for (_, canal) in PREFIXOS_DE_SESSAO {
            assert_eq!(
                remetente_da_superficie(canal),
                Remetente::Qualquer,
                "{canal}"
            );
        }
        let mut vistos = std::collections::HashSet::new();
        for (id, _) in SUPERFICIES {
            assert!(vistos.insert(*id), "{id} classificado duas vezes");
        }
    }

    /// A tabela inteira, escrita: so as superficies provadas locais sao do
    /// operador. O `mobile` fica do lado de fora — a conta e aberta — e o
    /// nome que ninguem classificou tambem.
    #[test]
    fn so_as_superficies_locais_sao_do_operador() {
        for local in ["web", "api", "vscode", "desktop", "cli", "mcp"] {
            assert_eq!(
                remetente_da_superficie(local),
                Remetente::Operador,
                "{local}"
            );
        }
        for remoto in [
            "mobile",
            "a2a",
            "openclaw",
            "whatsapp_linked",
            "whatsapp",
            "telegram",
            "discord",
            "slack",
            "google_chat",
            "teams",
            "line",
            "irc",
            "signal",
            "matrix",
            "imessage",
            "http",
            "api:agente",
            "",
            "superficie-nova",
        ] {
            assert_eq!(
                remetente_da_superficie(remoto),
                Remetente::Qualquer,
                "{remoto:?}"
            );
        }
    }

    #[test]
    fn porta_aberta_so_com_opt_out_sem_chave() {
        let mut gw = GatewayConfig::default();
        assert!(
            porta_do_operador_fechada(&gw),
            "default: loopback obrigatorio"
        );
        gw.allow_unauthenticated_network_bind = true;
        assert!(!porta_do_operador_fechada(&gw), "opt-out sem chave");
        gw.api_key = Some("uma-credencial-de-teste".to_string());
        assert!(
            porta_do_operador_fechada(&gw),
            "com chave, a porta e do operador"
        );
        gw.api_key = Some("   ".to_string());
        assert!(
            !porta_do_operador_fechada(&gw),
            "chave em branco nao e chave"
        );
    }

    #[test]
    fn ligado_na_config_segue_o_filtro_dos_builders() {
        let mut config = AppConfig::default();
        assert!(!ligado_na_config(&config, "telegram"));
        config.channels.insert(
            "meu-bot".to_string(),
            serde_json::from_value(serde_json::json!({ "type": "telegram" })).expect("canal"),
        );
        assert!(
            ligado_na_config(&config, "telegram"),
            "o nome da secao nao importa"
        );
        assert!(!ligado_na_config(&config, "discord"));
        config.channels.insert(
            "meu-bot".to_string(),
            serde_json::from_value(serde_json::json!({ "type": "telegram", "enabled": false }))
                .expect("canal"),
        );
        assert!(
            !ligado_na_config(&config, "telegram"),
            "desligado nao e ligado"
        );
    }
}
