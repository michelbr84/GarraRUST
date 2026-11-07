//! Histórico estruturado de ferramentas entre turnos (#1708).
//!
//! O `history` do chat levava só texto do usuário e do assistant; as chamadas
//! de ferramenta que o executor realmente rodou ficavam apenas no `ToolLog` da
//! UI. No turno seguinte o modelo não tinha como saber que `Write`/`Read`/`Bash`
//! rodaram e negava operações comprovadas.
//!
//! Aqui mora a trilha — alimentada por `TurnEvent`, que vem do executor, nunca
//! do LLM — e a conversão dela para mensagens no formato que os provedores
//! entendem:
//!
//! - assistant com blocos `ToolUse`;
//! - user com blocos `ToolResult` pareados por `call_id` (o `tool_use_id`).
//!
//! O modo é controlado por `GARRAIA_TOOL_HISTORY`:
//! - `structured` (padrão): histórico estruturado;
//! - `text`: comportamento antigo (resumo textual no histórico);
//! - `off`: nenhum histórico de ferramenta.
//!
//! Segurança: o input é conteúdo do modelo e pode carregar segredo. Em memória
//! ele fica como veio (já foi enviado ao provedor); na persistência passa por
//! redação de chaves sensíveis e de padrões de segredo, com teto de tamanho.
//! A saída da ferramenta é sempre dado não confiável e entra truncada.

use garraia_agents::{ChatMessage, ChatRole, ContentBlock, MessagePart};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Teto de caracteres por string persistida de input.
const INPUT_MAX_CHARS: usize = 2_048;
/// Teto de bytes da saída guardada por chamada.
const OUTPUT_MAX_BYTES: usize = 8 * 1024;
/// Quantas chamadas a trilha mantém em memória (as mais recentes).
const TRAIL_MAX_RECORDS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Ok,
    Error,
    Interrupted,
}

impl Status {
    #[cfg(test)]
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Interrupted => "interrupted",
        }
    }
}

/// Uma chamada de ferramenta observada no turno.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolRecord {
    pub call_id: String,
    pub name: String,
    pub input: Value,
    pub detail: String,
    pub status: Status,
    pub summary: String,
    pub output: String,
    pub turno: u32,
    pub ordem: u32,
}

/// Trilha viva da sessão. `observar` é chamado no dreno de eventos do turno.
#[derive(Debug, Default)]
pub struct ToolTrail {
    registros: Vec<ToolRecord>,
    turno: u32,
    ordem: u32,
}

impl ToolTrail {
    pub fn iniciar_turno(&mut self) {
        self.turno = self.turno.saturating_add(1);
        self.ordem = 0;
    }

    /// Registra o evento do executor. `ToolFinished` casa pelo `id`; se o
    /// início não chegou (sink de texto, por exemplo), o fim é ignorado — não
    /// inventamos uma chamada que não vimos começar.
    pub fn observar(&mut self, evento: &garraia_agents::TurnEvent) {
        match evento {
            garraia_agents::TurnEvent::ToolStarted {
                id,
                name,
                detail,
                input,
            } => {
                self.ordem = self.ordem.saturating_add(1);
                self.registros.push(ToolRecord {
                    call_id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                    detail: detail.clone(),
                    status: Status::Running,
                    summary: String::new(),
                    output: String::new(),
                    turno: self.turno,
                    ordem: self.ordem,
                });
                if self.registros.len() > TRAIL_MAX_RECORDS {
                    let excesso = self.registros.len() - TRAIL_MAX_RECORDS;
                    self.registros.drain(0..excesso);
                }
            }
            garraia_agents::TurnEvent::ToolFinished {
                id,
                success,
                summary,
                output,
                ..
            } => {
                if let Some(r) = self
                    .registros
                    .iter_mut()
                    .rev()
                    .find(|r| r.call_id == *id && r.status == Status::Running)
                {
                    r.status = if *success { Status::Ok } else { Status::Error };
                    r.summary = summary.clone();
                    r.output = output.clone();
                }
            }
            _ => {}
        }
    }

    /// Fecha o turno: chamadas ainda abertas viram `interrupted` — nunca
    /// sucesso — e devolve os registros do turno para o histórico/persistência.
    pub fn fim_do_turno(&mut self) -> Vec<ToolRecord> {
        for r in self.registros.iter_mut() {
            if r.turno == self.turno && r.status == Status::Running {
                r.status = Status::Interrupted;
                r.summary = "[interrompido]".to_string();
                r.output = "[interrompido: sem resultado]".to_string();
            }
        }
        self.registros
            .iter()
            .filter(|r| r.turno == self.turno)
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modo {
    Structured,
    Text,
    Off,
}

/// `GARRAIA_TOOL_HISTORY=structured|text|off`. Valor desconhecido cai no
/// padrão seguro (estruturado) e é registrado no stderr uma única vez.
pub fn modo() -> Modo {
    match std::env::var("GARRAIA_TOOL_HISTORY")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "text" => Modo::Text,
        "off" => Modo::Off,
        "structured" | "" => Modo::Structured,
        outro => {
            eprintln!("aviso: GARRAIA_TOOL_HISTORY={outro:?} desconhecido; usando structured");
            Modo::Structured
        }
    }
}

/// Converte as chamadas do turno em mensagens de histórico.
///
/// Devolve vazio quando não houve ferramenta. A ordem é sempre a que os
/// provedores exigem: assistant com `ToolUse` e, logo depois, user com os
/// `ToolResult` correspondentes — cada resultado referencia o id da chamada.
pub fn mensagens_de_ferramentas(registros: &[ToolRecord]) -> Vec<ChatMessage> {
    if registros.is_empty() {
        return Vec::new();
    }
    // Um par ToolUse/ToolResult por chamada, na ordem em que o executor
    // rodou. Assim `A -> resultado A -> B -> resultado B` continua sendo
    // ciclos sequenciais no historico: uma chamada dependente nunca aparece
    // como se tivesse rodado em paralelo com a anterior. Vale para os dois
    // contratos: a Anthropic aceita um bloco por mensagem e a OpenAI aceita
    // um `tool_calls` seguido do `role=tool` correspondente.
    let mut ordenados: Vec<&ToolRecord> = registros.iter().collect();
    ordenados.sort_by_key(|r| (r.turno, r.ordem));
    let mut mensagens = Vec::with_capacity(ordenados.len() * 2);
    for r in ordenados {
        mensagens.push(ChatMessage {
            role: ChatRole::Assistant,
            content: MessagePart::Parts(vec![ContentBlock::ToolUse {
                id: r.call_id.clone(),
                name: r.name.clone(),
                input: r.input.clone(),
            }]),
        });
        mensagens.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Parts(vec![ContentBlock::ToolResult {
                tool_use_id: r.call_id.clone(),
                content: conteudo_do_resultado(r),
            }]),
        });
    }
    mensagens
}

fn conteudo_do_resultado(r: &ToolRecord) -> String {
    let corpo = match r.status {
        Status::Ok => r.output.clone(),
        Status::Error => {
            if r.output.trim().is_empty() {
                format!("erro: {}", r.summary)
            } else {
                r.output.clone()
            }
        }
        Status::Interrupted => "[interrompido: sem resultado]".to_string(),
        Status::Running => "[sem resultado registrado]".to_string(),
    };
    let corpo = truncar_bytes(&corpo, OUTPUT_MAX_BYTES);
    if corpo.trim().is_empty() {
        if r.summary.trim().is_empty() {
            format!("({} executada, sem saida)", r.name)
        } else {
            r.summary.clone()
        }
    } else {
        corpo
    }
}

fn truncar_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut corte = max;
    while corte > 0 && !s.is_char_boundary(corte) {
        corte -= 1;
    }
    format!("{}\n[... truncado: {} bytes]", &s[..corte], s.len() - corte)
}

/// Versão persistível: input redigido e com teto, saída já capada.
pub fn para_persistencia(registros: &[ToolRecord]) -> Vec<ToolRecord> {
    registros
        .iter()
        .map(|r| {
            let mut input = redigir_valor(&r.input, None);
            if serde_json::to_string(&input).map(|s| s.len()).unwrap_or(0) > 16 * 1024 {
                input = serde_json::json!({ "resumo": truncar_chars(&r.detail, INPUT_MAX_CHARS) });
            }
            ToolRecord {
                input,
                output: redigir_texto_livre(&r.output, OUTPUT_MAX_BYTES),
                summary: redigir_texto_livre(&r.summary, 512),
                ..r.clone()
            }
        })
        .collect()
}

/// Texto livre vindo da ferramenta (saida, resumo): se carrega credencial,
/// vira marcador; senao passa pelo redactor e pelo teto de bytes.
fn redigir_texto_livre(s: &str, max_bytes: usize) -> String {
    if texto_com_credencial(s) {
        "[REDACTED]".to_string()
    } else {
        truncar_bytes(&garraia_security::redact_secrets(s), max_bytes)
    }
}

fn truncar_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cortado: String = s.chars().take(max).collect();
    format!("{cortado}…")
}

fn chave_sensivel(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    [
        "password",
        "secret",
        "token",
        "api_key",
        "apikey",
        "authorization",
        "cookie",
    ]
    .iter()
    .any(|p| k.contains(p))
}

/// Texto livre (comando, URL, header) que carrega credencial. Nesses casos a
/// string inteira vira marcador: preservar pedaços de um header e mais arriscado
/// do que perder o detalhe na persistência.
fn texto_com_credencial(s: &str) -> bool {
    let baixo = s.to_ascii_lowercase();
    [
        "authorization:",
        "bearer ",
        "x-api-key",
        "api_key=",
        "password=",
        "token=",
    ]
    .iter()
    .any(|p| baixo.contains(p))
}

fn redigir_valor(v: &Value, chave: Option<&str>) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, val)| (k.clone(), redigir_valor(val, Some(k.as_str()))))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(|x| redigir_valor(x, chave)).collect()),
        Value::String(s) => {
            if chave.map(chave_sensivel).unwrap_or(false) || texto_com_credencial(s) {
                Value::String("[REDACTED]".to_string())
            } else {
                Value::String(truncar_chars(
                    &garraia_security::redact_secrets(s),
                    INPUT_MAX_CHARS,
                ))
            }
        }
        outro => outro.clone(),
    }
}

/// Lê a trilha gravada no metadata da mensagem do assistant. Metadados antigos
/// (sem a chave) devolvem vazio — compatível com sessões existentes.
pub fn do_metadata(meta: &Value) -> Vec<ToolRecord> {
    meta.get("tool_trail")
        .and_then(|v| serde_json::from_value::<Vec<ToolRecord>>(v.clone()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod testes {
    use super::*;

    fn registro(id: &str, nome: &str, status: Status) -> ToolRecord {
        ToolRecord {
            call_id: id.to_string(),
            name: nome.to_string(),
            input: serde_json::json!({ "path": "pagina.html" }),
            detail: "pagina.html".to_string(),
            status,
            summary: "ok".to_string(),
            output: "conteudo".to_string(),
            turno: 1,
            ordem: 1,
        }
    }

    #[test]
    fn pareamento_usa_o_mesmo_id_no_use_e_no_result() {
        let msgs = mensagens_de_ferramentas(&[registro("call-1", "file_write", Status::Ok)]);
        assert_eq!(msgs.len(), 2);
        let MessagePart::Parts(usos) = &msgs[0].content else {
            panic!("esperava Parts");
        };
        let MessagePart::Parts(resultados) = &msgs[1].content else {
            panic!("esperava Parts");
        };
        let ContentBlock::ToolUse { id, .. } = &usos[0] else {
            panic!("esperava ToolUse");
        };
        let ContentBlock::ToolResult { tool_use_id, .. } = &resultados[0] else {
            panic!("esperava ToolResult");
        };
        assert_eq!(id, "call-1");
        assert_eq!(tool_use_id, id);
        assert_eq!(msgs[0].role, ChatRole::Assistant);
        assert_eq!(msgs[1].role, ChatRole::User);
    }

    #[test]
    fn chamada_interrompida_nunca_vira_sucesso() {
        let mut trail = ToolTrail::default();
        trail.iniciar_turno();
        trail.observar(&garraia_agents::TurnEvent::ToolStarted {
            id: "call-9".into(),
            name: "bash".into(),
            detail: "cargo test".into(),
            input: serde_json::json!({ "command": "cargo test" }),
        });
        let registros = trail.fim_do_turno();
        assert_eq!(registros[0].status, Status::Interrupted);
        assert_eq!(registros[0].summary, "[interrompido]");
        let msgs = mensagens_de_ferramentas(&registros);
        let MessagePart::Parts(resultados) = &msgs[1].content else {
            panic!("esperava Parts");
        };
        let ContentBlock::ToolResult { content, .. } = &resultados[0] else {
            panic!("esperava ToolResult");
        };
        assert!(content.contains("interrompido"));
    }

    #[test]
    fn fim_sem_inicio_nao_inventa_chamada() {
        let mut trail = ToolTrail::default();
        trail.iniciar_turno();
        trail.observar(&garraia_agents::TurnEvent::ToolFinished {
            id: "orfao".into(),
            name: "bash".into(),
            duration: std::time::Duration::ZERO,
            success: true,
            summary: "ok".into(),
            output: "x".into(),
        });
        assert!(trail.fim_do_turno().is_empty());
    }

    #[test]
    fn ordem_de_ciclos_sequenciais_e_preservada() {
        let mut a = registro("call-a", "file_write", Status::Ok);
        a.ordem = 1;
        a.output = "a ok".to_string();
        let mut b = registro("call-b", "file_read", Status::Ok);
        b.ordem = 2;
        b.output = "b ok".to_string();

        // Entra fora de ordem de proposito: a saida tem de sair ordenada.
        let msgs = mensagens_de_ferramentas(&[b, a]);
        assert_eq!(msgs.len(), 4, "um par por chamada, nunca um bloco unico");
        assert_eq!(
            msgs.iter().map(|m| m.role.clone()).collect::<Vec<_>>(),
            vec![
                ChatRole::Assistant,
                ChatRole::User,
                ChatRole::Assistant,
                ChatRole::User
            ]
        );
        let ids: Vec<String> = msgs
            .iter()
            .map(|m| match &m.content {
                MessagePart::Parts(b) => match &b[0] {
                    ContentBlock::ToolUse { id, .. } => format!("use:{id}"),
                    ContentBlock::ToolResult { tool_use_id, .. } => format!("res:{tool_use_id}"),
                    _ => "outro".to_string(),
                },
                MessagePart::Text(t) => format!("txt:{t}"),
            })
            .collect();
        assert_eq!(
            ids,
            vec![
                "use:call-a".to_string(),
                "res:call-a".to_string(),
                "use:call-b".to_string(),
                "res:call-b".to_string()
            ]
        );
    }

    #[test]
    fn persistencia_redige_saida_e_resumo_com_credencial() {
        let segredo = format!("Bearer {}", concat!("tok", "en-987"));
        let mut r = registro("call-3", "bash", Status::Ok);
        r.output = format!("HTTP/1.1 200\nAuthorization: {segredo}\nok");
        r.summary = format!("enviou Authorization: {segredo}");
        let p = para_persistencia(&[r]);
        assert_eq!(p[0].output, "[REDACTED]");
        assert_eq!(p[0].summary, "[REDACTED]");
        assert!(!p[0].output.contains(&segredo));
    }

    #[test]
    fn persistencia_redige_segredo_e_mantem_estrutura() {
        // O valor e montado em runtime para o proprio teste nao carregar um
        // token literal no fonte.
        let segredo = format!("Bearer {}", concat!("abc", "123"));
        let mut r = registro("call-2", "bash", Status::Ok);
        r.input = serde_json::json!({
            "command": format!("curl -H 'Authorization: {segredo}'"),
            "token": "segredo"
        });
        let p = para_persistencia(&[r]);
        assert_eq!(p[0].input["token"], "[REDACTED]");
        let cmd = p[0].input["command"].as_str().unwrap();
        assert!(!cmd.contains(&segredo), "segredo vazou: {cmd}");
    }

    #[test]
    fn metadados_antigos_nao_quebram() {
        assert!(do_metadata(&serde_json::json!({})).is_empty());
        assert!(do_metadata(&serde_json::json!({ "tool_trail": "lixo" })).is_empty());
    }

    #[test]
    fn status_serializa_em_minusculas() {
        let j = serde_json::to_string(&Status::Interrupted).unwrap();
        assert_eq!(j, "\"interrupted\"");
        assert_eq!(Status::Ok.as_str(), "ok");
    }
}
