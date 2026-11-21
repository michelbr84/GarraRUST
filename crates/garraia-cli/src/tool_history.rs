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

impl ToolRecord {
    /// Sem resultado observado: o turno caiu com a chamada em andamento.
    fn marcar_interrompida(&mut self) {
        self.status = Status::Interrupted;
        self.summary = "[interrompido]".to_string();
        self.output = "[interrompido: sem resultado]".to_string();
    }
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
                outcome,
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
                    // #1915: o runtime fecha com `Cancelada` a chamada cujo
                    // turno caiu no meio. Para o historico e a mesma coisa que
                    // o `fim_do_turno` faz com uma chamada ainda aberta: sem
                    // resultado observado, nunca erro nem sucesso. O mesmo
                    // predicado decide o ledger de evidencia do runtime.
                    if !outcome.resultado_observado() {
                        r.marcar_interrompida();
                    } else {
                        r.status = if *success { Status::Ok } else { Status::Error };
                        r.summary = summary.clone();
                        r.output = output.clone();
                    }
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
                r.marcar_interrompida();
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

/// Redação do texto que vai para a SUPERFÍCIE de resposta (#1709 fatia 3).
///
/// Combina as duas defesas numa ordem fixa e testável:
/// 1. [`redigir_texto_livre`] — credencial/marca de segredo vira `[REDACTED]`
///    (o `redact_secrets` ainda apanha formatos de terceiro que sobram);
/// 2. [`garraia_security::sanitize_indirect`] — remove caracteres invisíveis
///    (o único saneamento seguro a aplicar às cegas) e sinaliza padrão de
///    injeção de prompt; sinalizado, o texto sai com o marcador de dado
///    não confiável.
///
/// O texto da `Claim` pode ecoar o que uma ferramenta de terceiro devolveu;
/// a superfície de resposta não o repete como se fosse instrução nem deixa
/// credencial escapar.
fn sanear_superficie(s: &str) -> String {
    let sem_credencial = redigir_texto_livre(s, SUPERFICIE_MAX_BYTES);
    let (limpo, report) = garraia_security::sanitize_indirect(&sem_credencial);
    if report.is_suspicious() {
        format!("{} [dado nao confiavel]", limpo)
    } else {
        limpo
    }
}

/// Teto de bytes do texto projetado na superfície de resposta. Curto de
/// propósito: o sumário é curto por definição (uma linha por afirmação), e um
/// teto pequeno limita o que um texto hostil consegue empurrar para a tela.
const SUPERFICIE_MAX_BYTES: usize = 512;

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

/// Projeta a trilha de ferramentas num sumário curto com proveniência
/// (#1709 fatia 2), pronto para a camada de resposta (CLI/canais).
///
/// Cada [`ToolRecord`] vira uma [`Claim`] com a classe de evidência adequada:
/// `Ok` → [`VerificationStatus::Observado`], `Error` → `Falhou`,
/// `Interrupted` → `NaoVerificado` (nunca sucesso), `Running` → `NaoVerificado`
/// (sem resultado registrado). A [`EvidenceRef`] aponta para o `call_id` real
/// do executor — nunca para o texto do modelo.
///
/// #1709 fatia 3 (segurança da superfície de resposta): o `texto` da `Claim`
/// deriva de `detail`/`name` da ferramenta — dado que o executor registrou, mas
/// que pode ecoar conteúdo de terceiros. Antes de projetar, o texto passa por
/// [`sanear_superficie`] (redação de credencial + saneamento de injeção de
/// prompt), para a superfície de resposta nunca divulgar credencial nem
/// amplificar instrução embutida em saída de ferramenta.
pub fn resumo_de_claims(registros: &[ToolRecord]) -> garraia_agents::SurfaceSummary {
    use garraia_agents::{Claim, EvidenceRef};
    let claims: Vec<Claim> = registros
        .iter()
        .map(|r| {
            let evidencia = EvidenceRef {
                tool_call_id: r.call_id.clone(),
                tool_name: r.name.clone(),
                success: r.status == Status::Ok,
                turno: r.turno,
                ordem: r.ordem,
            };
            let cru = if r.detail.trim().is_empty() {
                r.name.clone()
            } else {
                format!("{}: {}", r.name, r.detail)
            };
            let texto = sanear_superficie(&cru);
            match r.status {
                Status::Ok => Claim::observada(texto, evidencia),
                Status::Error => Claim::falhou(texto, evidencia),
                // Interrupted e Running: sem resultado comprovado — nunca
                // sucesso nem "não aconteceu".
                Status::Interrupted | Status::Running => Claim::nao_verificada(texto),
            }
        })
        .collect();
    garraia_agents::SurfaceSummary::de_claims(&claims)
}

#[cfg(test)]
mod testes;
