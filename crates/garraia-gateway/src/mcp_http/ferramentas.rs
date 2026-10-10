//! Os descritores das seis tools e a validacao dos argumentos delas.
//!
//! Puro: nenhuma funcao daqui toca `AppState`, rede ou disco. O que os
//! descritores anunciam e o que [`super::handler`] de fato aceita tem de ser a
//! mesma coisa, e a forma de garantir isso sem um servidor de pe e manter as
//! duas metades no mesmo arquivo — o schema logo acima do `struct` que o
//! desserializa.
//!
//! ## `deny_unknown_fields` nao e decoracao
//!
//! Todo schema aqui leva `additionalProperties: false` e todo `struct` leva
//! `#[serde(deny_unknown_fields)]`. Em MCP o schema e **consultivo**: o host
//! pode mandar o que quiser, e um campo a mais que o servidor ignora em
//! silencio e um pedido que o chamador acha que fez e o servidor acha que
//! nao. Num `garra_send_message` isso seria uma mensagem indo para um destino
//! diferente do pedido.

use std::sync::Arc;

use garraia_ask::{
    ARG_MESSAGE_MAX_BYTES, ARG_SYSTEM_PROMPT_MAX_BYTES, ARG_TIMEOUT_SECS_DEFAULT,
    ARG_TIMEOUT_SECS_MAX, ARG_TIMEOUT_SECS_MIN, PROVEDORES_ASK,
};
use garraia_config::defaults::{DEFAULT_CLOUD_MODEL, DEFAULT_CLOUD_PROVIDER};
use rmcp::model::{Tool, ToolAnnotations};
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue, json};

use super::politica::{CANAL_DE_ENVIO, PoliticaMcpHttp};

/// Teto do texto de um envio, em caracteres.
///
/// Mesmo valor do `telegram_send` (`tools/channel_send_tool.rs`): o Telegram
/// corta em 4096 unidades UTF-16, e recusar aqui com um motivo legivel e melhor
/// que uma rejeicao da Bot API que o chamador nao sabe interpretar.
pub const MAX_TEXTO_CHARS: usize = 4000;

/// Nomes das cinco tools, em um lugar so.
///
/// Usados pelo descritor, pelo despacho e pelos testes. O despacho e um `match`
/// sobre estas constantes justamente para que renomear uma tool sem atualizar o
/// anuncio nao compile.
pub const TOOL_STATUS: &str = "garra_status";
pub const TOOL_LIST_CHATS: &str = "garra_list_chats";
pub const TOOL_READ_HISTORY: &str = "garra_read_history";
pub const TOOL_SEND_MESSAGE: &str = "garra_send_message";
pub const TOOL_PAIR_STATUS: &str = "garra_pair_status";
/// #1612 — a sexta tool: inferencia sob demanda, anunciada so com `allow_ask`.
pub const TOOL_ASK: &str = "garra_ask";

/// Todas as tools que a ponte conhece, pelo nome `garra_*`.
const TODAS_AS_TOOLS: [&str; 6] = [
    TOOL_STATUS,
    TOOL_LIST_CHATS,
    TOOL_READ_HISTORY,
    TOOL_PAIR_STATUS,
    TOOL_SEND_MESSAGE,
    TOOL_ASK,
];

/// A tool `garra_*` para o nome curto que o operador escreve em
/// `gateway.mcp_http.orchestrators[].tools` (#1613). `None` para um nome que a
/// ponte nao tem: a entrada nao concede nada por ele.
pub fn tool_por_nome_curto(curto: &str) -> Option<&'static str> {
    match curto {
        "status" => Some(TOOL_STATUS),
        "list_chats" => Some(TOOL_LIST_CHATS),
        "read_history" => Some(TOOL_READ_HISTORY),
        "pair_status" => Some(TOOL_PAIR_STATUS),
        "send_message" => Some(TOOL_SEND_MESSAGE),
        "ask" => Some(TOOL_ASK),
        _ => None,
    }
}

/// O nome pedido e uma tool da ponte? Para o log: um nome qualquer vindo de
/// quem chama nao entra nele como esta.
pub fn e_ferramenta_conhecida(nome: &str) -> bool {
    TODAS_AS_TOOLS.contains(&nome)
}

/// `serde_json::Value::Object` garantido pelos literais abaixo.
fn objeto(v: JsonValue) -> Arc<JsonMap<String, JsonValue>> {
    match v {
        JsonValue::Object(map) => Arc::new(map),
        _ => unreachable!("todo schema deste modulo e um literal de objeto"),
    }
}

/// Schema de uma tool que nao recebe argumento nenhum.
fn sem_argumentos() -> Arc<JsonMap<String, JsonValue>> {
    objeto(json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    }))
}

/// As anotacoes das quatro tools de leitura.
///
/// `read_only_hint` e um **hint** na spec do MCP — o host nao e obrigado a
/// respeita-lo, e por isso ele nao substitui trava nenhuma deste modulo. Ele
/// serve para o outro lado: um orquestrador que sabe distinguir leitura de
/// escrita pode chamar as quatro sem cerimonia e pedir confirmacao so na
/// quinta, que e exatamente o comportamento que se quer induzir.
fn somente_leitura() -> ToolAnnotations {
    ToolAnnotations::new().read_only(true)
}

fn tool_status() -> Tool {
    Tool::new(
        TOOL_STATUS,
        "Saude deste Garra: versao, uptime, canais e contagem de conversas em memoria. \
         Somente leitura.",
        sem_argumentos(),
    )
    .with_annotations(somente_leitura())
}

fn tool_list_chats() -> Tool {
    Tool::new(
        TOOL_LIST_CHATS,
        "Lista as conversas que este Garra conhece agora, com canal, tamanho do historico \
         e inatividade. Somente leitura. Use o campo `chat` de uma linha como argumento de \
         garra_read_history.",
        objeto(json!({
            "type": "object",
            "properties": {
                "channel": {
                    "type": "string",
                    "description": "Filtra por canal (`telegram`, `web`, `mobile`, ...). \
                                    Omita para listar todos.",
                    "minLength": 1,
                    "maxLength": 64
                }
            },
            "additionalProperties": false
        })),
    )
    .with_annotations(somente_leitura())
}

fn tool_read_history() -> Tool {
    Tool::new(
        TOOL_READ_HISTORY,
        "Ultimas mensagens de uma conversa, com segredos redigidos. Somente leitura. \
         O `chat` e o identificador que garra_list_chats devolve.",
        objeto(json!({
            "type": "object",
            "properties": {
                "chat": {
                    "type": "string",
                    "description": "Identificador da conversa, vindo de garra_list_chats.",
                    "minLength": 1,
                    "maxLength": 256
                },
                "limit": {
                    "type": "integer",
                    "description": "Quantas mensagens do fim da conversa. Acima do teto do \
                                    operador, vale o teto.",
                    "minimum": 1
                }
            },
            "required": ["chat"],
            "additionalProperties": false
        })),
    )
    .with_annotations(somente_leitura())
}

fn tool_send_message() -> Tool {
    Tool::new(
        TOOL_SEND_MESSAGE,
        "Envia uma mensagem num canal real, em nome do operador. Só alcança destinos que o \
         operador liberou na config — não há como aprovar um destino novo por aqui.",
        objeto(json!({
            "type": "object",
            "properties": {
                "channel": {
                    "type": "string",
                    "enum": [CANAL_DE_ENVIO],
                    "description": "Canal de saida. Hoje so `telegram`."
                },
                "chat_id": {
                    "type": "integer",
                    "description": "Chat de destino. Precisa estar em \
                                    `channels.<canal>.proactive_chat_ids`."
                },
                "text": {
                    "type": "string",
                    "description": "Texto da mensagem.",
                    "minLength": 1,
                    "maxLength": MAX_TEXTO_CHARS
                }
            },
            "required": ["channel", "chat_id", "text"],
            "additionalProperties": false
        })),
    )
    // Nao e leitura, nao e idempotente (duas chamadas iguais = duas mensagens
    // na conversa de alguem) e alcanca o mundo de fora. As tres coisas ditas em
    // voz alta, para o host do outro lado tratar esta tool diferente das quatro.
    .with_annotations(ToolAnnotations::from_raw(
        None,
        Some(false),
        Some(false),
        Some(false),
        Some(true),
    ))
}

/// `garra_ask` (#1612): o mesmo schema da tool do stdio, e o mesmo
/// `garra.ask.v1` de volta. `additionalProperties: false` e `deny_unknown_fields`
/// valem aqui pelo mesmo motivo do envio.
fn tool_ask() -> Tool {
    let schema = json!({
        "type": "object",
        "properties": {
            "message": {
                "type": "string",
                "description": "A pergunta ou instrucao para o GarraIA. Ate 64 KiB.",
                "minLength": 1,
                "maxLength": ARG_MESSAGE_MAX_BYTES
            },
            "provider": {
                "type": "string",
                "enum": provedores_anunciados(),
                "default": DEFAULT_CLOUD_PROVIDER,
                "description": format!("Provider de LLM. Padrao '{DEFAULT_CLOUD_PROVIDER}'.")
            },
            "model": {
                "type": "string",
                "default": DEFAULT_CLOUD_MODEL,
                "description": "Modelo. So os modelos que o operador liberou na config \
                                sao aceitos; o padrao e o modelo do projeto."
            },
            "timeout_secs": {
                "type": "integer",
                "default": ARG_TIMEOUT_SECS_DEFAULT,
                "minimum": ARG_TIMEOUT_SECS_MIN,
                "maximum": ARG_TIMEOUT_SECS_MAX,
                "description": "Timeout da chamada ao LLM, em segundos."
            },
            "system_prompt": {
                "type": "string",
                "maxLength": ARG_SYSTEM_PROMPT_MAX_BYTES,
                "description": "Substitui o prompt de sistema padrao, se informado."
            }
        },
        "required": ["message"],
        "additionalProperties": false
    });
    Tool::new(
        TOOL_ASK,
        "Faz uma pergunta ao GarraIA, que chama o LLM configurado. Sem shell, arquivo ou \
         git. Gasta a chave de provider do operador: use quando a tarefa precisa de \
         inferencia de fato. Devolve um envelope `garra.ask.v1`.",
        objeto(schema),
    )
    // Nao e leitura, nao e idempotente (cada chamada gasta inferencia) e alcanca
    // um provider externo. As mesmas tres coisas ditas em voz alta do envio.
    .with_annotations(ToolAnnotations::from_raw(
        None,
        Some(false),
        Some(false),
        Some(false),
        Some(true),
    ))
}

/// Os providers que o schema anuncia. O `echo` so existe em build de
/// desenvolvimento (feature `dev-echo-provider`), e so aparece la.
fn provedores_anunciados() -> Vec<&'static str> {
    PROVEDORES_ASK
        .into_iter()
        .chain(cfg!(feature = "dev-echo-provider").then_some("echo"))
        .collect()
}

fn tool_pair_status() -> Tool {
    Tool::new(
        TOOL_PAIR_STATUS,
        "Estado de pareamento dos canais: quais estao ligados, quais esperam segredo e \
         quais estao no ar. Somente leitura.",
        sem_argumentos(),
    )
    .with_annotations(somente_leitura())
}

/// A superficie anunciada em `tools/list`.
///
/// Quatro tools de leitura sempre; `garra_send_message` entra somente quando a
/// politica diz que um envio poderia sair (ver
/// [`PoliticaMcpHttp::anuncia_envio`](super::politica::PoliticaMcpHttp::anuncia_envio));
/// `garra_ask` entra somente com `allow_ask` (#1612). Puro de proposito: um
/// teste fixa a superficie sem subir servidor.
pub fn tools_anunciadas(politica: &PoliticaMcpHttp) -> Vec<Tool> {
    let mut tools = vec![
        tool_status(),
        tool_list_chats(),
        tool_read_history(),
        tool_pair_status(),
    ];
    if politica.anuncia_envio() {
        tools.push(tool_send_message());
    }
    if politica.anuncia_ask() {
        tools.push(tool_ask());
    }
    tools
}

/// Argumentos de `garra_list_chats`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArgsListChats {
    #[serde(default)]
    pub channel: Option<String>,
}

/// Argumentos de `garra_read_history`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArgsReadHistory {
    pub chat: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Argumentos de `garra_send_message`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArgsSendMessage {
    pub channel: String,
    pub chat_id: i64,
    pub text: String,
}

/// Argumentos de `garra_ask` (#1612). Mesmo formato do stdio.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArgsAsk {
    pub message: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub system_prompt: Option<String>,
}

/// Validacao de limites de `garra_ask`. Forma e tamanho apenas: quem decide se
/// a chamada roda e [`PoliticaMcpHttp::decidir_ask`](super::politica::PoliticaMcpHttp::decidir_ask).
///
/// O `provider` e conferido contra o enum anunciado, e nao so pelo schema:
/// hosts MCP tratam o schema como consultivo, e o stdio ja o aplica aqui.
pub fn validar_ask(args: &ArgsAsk) -> Result<(), String> {
    garraia_ask::validar_argumentos(
        &args.message,
        args.timeout_secs,
        args.system_prompt.as_deref(),
    )?;
    if let Some(provider) = args.provider.as_deref()
        && !provedores_anunciados().contains(&provider)
    {
        return Err(format!(
            "provider '{provider}' nao e aceito por garra_ask (aceitos: {})",
            provedores_anunciados().join(", ")
        ));
    }
    if let Some(model) = args.model.as_deref()
        && model.trim().is_empty()
    {
        return Err("`model` esta vazio".to_string());
    }
    Ok(())
}

/// Validacao de limites de `garra_read_history`. O `Err` e a mensagem que o
/// chamador recebe.
pub fn validar_read_history(args: &ArgsReadHistory) -> Result<(), String> {
    if args.chat.trim().is_empty() {
        return Err("`chat` esta vazio".to_string());
    }
    Ok(())
}

/// Validacao de limites de `garra_send_message`.
///
/// So forma e tamanho — **nao** autorizacao. Quem decide se o envio sai e
/// [`PoliticaMcpHttp::decidir_envio`](super::politica::PoliticaMcpHttp::decidir_envio),
/// e essa ordem importa: validar depois de decidir transformaria a mensagem de
/// erro num oraculo ("esse destino nao vale" versus "esse texto e longo demais"
/// contam coisas diferentes sobre a allowlist).
pub fn validar_send_message(args: &ArgsSendMessage) -> Result<(), String> {
    if args.text.trim().is_empty() {
        return Err("`text` esta vazio".to_string());
    }
    let chars = args.text.chars().count();
    if chars > MAX_TEXTO_CHARS {
        return Err(format!(
            "mensagem muito longa: {chars} caracteres (limite: {MAX_TEXTO_CHARS})"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel_send::ProactiveTargets;

    fn politica(envio: bool, ids: &[i64]) -> PoliticaMcpHttp {
        PoliticaMcpHttp::nova(envio, ProactiveTargets::from_ids(ids.iter().copied()))
    }

    /// Os nomes anunciados, na ordem em que o `tools/list` os devolve.
    fn nomes(envio: bool, ids: &[i64]) -> Vec<String> {
        tools_anunciadas(&politica(envio, ids))
            .into_iter()
            .map(|t| t.name.to_string())
            .collect()
    }

    /// No default da instalacao a superficie tem as quatro de leitura, e
    /// `garra_send_message` nao aparece.
    #[test]
    fn superficie_default_e_so_leitura() {
        assert_eq!(
            nomes(false, &[]),
            vec![
                TOOL_STATUS,
                TOOL_LIST_CHATS,
                TOOL_READ_HISTORY,
                TOOL_PAIR_STATUS
            ]
        );
    }

    /// Com o operador tendo ligado a chave E liberado um destino, as cinco da
    /// spec aparecem.
    #[test]
    fn superficie_liberada_tem_as_cinco() {
        let nomes = nomes(true, &[42]);
        assert_eq!(nomes.len(), 5, "{nomes:?}");
        assert!(nomes.iter().any(|n| n == TOOL_SEND_MESSAGE));
    }

    /// Interruptor ligado mas allowlist vazia: nada pode sair, e a tool
    /// tampouco e oferecida.
    #[test]
    fn sem_allowlist_a_tool_de_envio_nao_e_anunciada() {
        let nomes = nomes(true, &[]);
        assert!(!nomes.iter().any(|n| n == TOOL_SEND_MESSAGE), "{nomes:?}");
    }

    /// Todo schema anunciado fecha `additionalProperties`. Sem isso, o
    /// `deny_unknown_fields` do lado Rust recusaria um pedido que o schema
    /// dizia ser valido — divergencia entre anuncio e aceitacao.
    #[test]
    fn todo_schema_fecha_additional_properties() {
        for t in tools_anunciadas(&politica(true, &[1])) {
            let schema = JsonValue::Object((*t.input_schema).clone());
            assert_eq!(
                schema["additionalProperties"],
                json!(false),
                "{}: {schema}",
                t.name
            );
            assert_eq!(schema["type"], json!("object"), "{}", t.name);
        }
    }

    /// Toda tool tem descricao nao vazia: e o unico texto pelo qual o modelo do
    /// outro lado decide se chama ou nao.
    #[test]
    fn toda_tool_tem_descricao() {
        for t in tools_anunciadas(&politica(true, &[1])) {
            let d = t.description.as_deref().unwrap_or_default();
            assert!(!d.trim().is_empty(), "{} sem descricao", t.name);
        }
    }

    /// O enum de `channel` do schema nomeia o mesmo canal que a politica
    /// aceita. Divergirem seria anunciar um canal que todo envio recusa.
    #[test]
    fn schema_de_envio_anuncia_o_canal_que_a_politica_aceita() {
        let t = tool_send_message();
        let schema = JsonValue::Object((*t.input_schema).clone());
        assert_eq!(
            schema["properties"]["channel"]["enum"],
            json!([CANAL_DE_ENVIO])
        );
    }

    #[test]
    fn campo_desconhecido_e_recusado() {
        let erro = serde_json::from_value::<ArgsSendMessage>(json!({
            "channel": "telegram",
            "chat_id": 1,
            "text": "oi",
            "chat_id_de_verdade": 2
        }));
        assert!(erro.is_err(), "campo extra passou");
    }

    #[test]
    fn texto_vazio_e_longo_demais_sao_recusados() {
        let base = |texto: String| ArgsSendMessage {
            channel: CANAL_DE_ENVIO.to_string(),
            chat_id: 1,
            text: texto,
        };
        assert!(validar_send_message(&base("   ".into())).is_err());
        assert!(validar_send_message(&base("a".repeat(MAX_TEXTO_CHARS + 1))).is_err());
        assert!(validar_send_message(&base("a".repeat(MAX_TEXTO_CHARS))).is_ok());
    }

    /// Contado em `chars` e nao em bytes: um texto de emoji no limite passa.
    #[test]
    fn limite_de_texto_e_por_caractere_nao_por_byte() {
        let args = ArgsSendMessage {
            channel: CANAL_DE_ENVIO.to_string(),
            chat_id: 1,
            text: "🦀".repeat(MAX_TEXTO_CHARS),
        };
        assert!(validar_send_message(&args).is_ok());
    }

    fn com_ask() -> PoliticaMcpHttp {
        politica(false, &[]).com_ask(&[])
    }

    /// #1612 — `garra_ask` so entra na superficie com `allow_ask`. No default,
    /// a superficie continua sendo a de antes.
    #[test]
    fn garra_ask_so_aparece_com_allow_ask() {
        let padrao = nomes(false, &[]);
        assert!(!padrao.iter().any(|n| n == TOOL_ASK), "{padrao:?}");

        let ligada: Vec<String> = tools_anunciadas(&com_ask())
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        assert!(ligada.iter().any(|n| n == TOOL_ASK), "{ligada:?}");
        assert_eq!(ligada.len(), 5, "{ligada:?}");
    }

    /// O schema de `garra_ask` fecha os campos e exige so a mensagem — o mesmo
    /// contrato do stdio, que e o que o orquestrador ja conhece.
    #[test]
    fn schema_de_ask_fecha_campos_e_exige_message() {
        let t = tools_anunciadas(&com_ask())
            .into_iter()
            .find(|t| t.name == TOOL_ASK)
            .expect("garra_ask anunciada");
        let schema = JsonValue::Object((*t.input_schema).clone());
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["message"]));
        assert_eq!(
            schema["properties"]["timeout_secs"]["maximum"],
            json!(ARG_TIMEOUT_SECS_MAX)
        );
        assert_eq!(
            schema["properties"]["system_prompt"]["maxLength"],
            json!(ARG_SYSTEM_PROMPT_MAX_BYTES)
        );
        assert_eq!(
            schema["properties"]["model"]["default"],
            json!(DEFAULT_CLOUD_MODEL)
        );
    }

    /// Campo a mais no `garra_ask` e recusado pelo `deny_unknown_fields`.
    #[test]
    fn ask_campo_desconhecido_e_recusado() {
        let erro = serde_json::from_value::<ArgsAsk>(json!({
            "message": "oi",
            "modelo": "openrouter/auto"
        }));
        assert!(erro.is_err(), "campo extra passou");
    }

    fn ask(message: &str) -> ArgsAsk {
        ArgsAsk {
            message: message.to_string(),
            provider: None,
            model: None,
            timeout_secs: None,
            system_prompt: None,
        }
    }

    #[test]
    fn validar_ask_aceita_o_pedido_minimo() {
        assert!(validar_ask(&ask("oi")).is_ok());
    }

    #[test]
    fn validar_ask_recusa_mensagem_vazia_ou_grande_demais() {
        assert!(validar_ask(&ask("   ")).is_err());
        assert!(validar_ask(&ask(&"a".repeat(ARG_MESSAGE_MAX_BYTES + 1))).is_err());
    }

    #[test]
    fn validar_ask_recusa_timeout_fora_da_faixa() {
        let mut a = ask("oi");
        a.timeout_secs = Some(0);
        assert!(validar_ask(&a).is_err());
        a.timeout_secs = Some(ARG_TIMEOUT_SECS_MAX + 1);
        assert!(validar_ask(&a).is_err());
        a.timeout_secs = Some(ARG_TIMEOUT_SECS_MAX);
        assert!(validar_ask(&a).is_ok());
    }

    /// O provider e conferido contra o enum, nao so pelo schema — um alias de
    /// `llm:` ou um nome qualquer nao chega ao provider.
    #[test]
    fn validar_ask_recusa_provider_fora_do_enum() {
        let mut a = ask("oi");
        a.provider = Some("minha-entrada-llm".to_string());
        assert!(validar_ask(&a).is_err());
        a.provider = Some("openai".to_string());
        assert!(validar_ask(&a).is_ok());
    }

    #[test]
    fn validar_ask_recusa_modelo_vazio() {
        let mut a = ask("oi");
        a.model = Some("   ".to_string());
        assert!(validar_ask(&a).is_err());
    }

    #[test]
    fn chat_vazio_no_historico_e_recusado() {
        let args = ArgsReadHistory {
            chat: "  ".into(),
            limit: None,
        };
        assert!(validar_read_history(&args).is_err());
    }

    /// #1613 — cada nome que a config aceita traduz para uma tool da ponte, e
    /// as duas listas sao a mesma: nenhuma tool fica sem nome curto.
    #[test]
    fn nomes_curtos_da_config_cobrem_as_tools_da_ponte() {
        use garraia_config::FERRAMENTAS_DE_ORQUESTRADOR;
        for curto in FERRAMENTAS_DE_ORQUESTRADOR {
            assert!(tool_por_nome_curto(curto).is_some(), "{curto} sem tool");
        }
        let traduzidos: std::collections::BTreeSet<&str> = FERRAMENTAS_DE_ORQUESTRADOR
            .iter()
            .filter_map(|c| tool_por_nome_curto(c))
            .collect();
        assert_eq!(traduzidos.len(), TODAS_AS_TOOLS.len(), "{traduzidos:?}");
        for tool in TODAS_AS_TOOLS {
            assert!(traduzidos.contains(tool), "{tool} sem nome curto");
        }
    }

    #[test]
    fn nome_com_prefixo_ou_inventado_nao_e_tool() {
        assert_eq!(tool_por_nome_curto("garra_status"), None);
        assert_eq!(tool_por_nome_curto("formata_o_disco"), None);
        assert!(e_ferramenta_conhecida(TOOL_ASK));
        assert!(!e_ferramenta_conhecida("formata_o_disco"));
    }
}
