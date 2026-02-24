use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Agent Card conforme definido pelo protocolo A2A.
/// Exposto em `/.well-known/agent.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    /// Nome do agente.
    pub name: String,

    /// Descrição opcional do agente.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// URL base do agente.
    pub url: String,

    /// Versão opcional do agente.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Capacidades suportadas pelo agente.
    #[serde(default)]
    pub capabilities: AgentCapabilities,

    /// Lista de habilidades expostas pelo agente.
    #[serde(default)]
    pub skills: Vec<AgentSkill>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    /// Indica se o agente suporta streaming de respostas.
    #[serde(default)]
    pub streaming: bool,

    /// Indica se o agente suporta notificações push.
    #[serde(default)]
    pub push_notifications: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    /// Identificador único da habilidade.
    pub id: String,

    /// Nome da habilidade.
    pub name: String,

    /// Descrição opcional da habilidade.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Tags associadas à habilidade.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Uma tarefa A2A representando uma unidade de trabalho entre agentes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2ATask {
    /// Identificador da tarefa.
    pub id: String,

    /// Status atual da tarefa.
    pub status: TaskStatus,

    /// Mensagens trocadas no contexto da tarefa.
    #[serde(default)]
    pub messages: Vec<A2AMessage>,

    /// Artefatos produzidos durante a execução da tarefa.
    #[serde(default)]
    pub artifacts: Vec<A2AArtifact>,

    /// Metadados adicionais associados à tarefa.
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Status do ciclo de vida da tarefa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Submitted,
    Working,
    Completed,
    Failed,
    Canceled,
}

/// Uma mensagem dentro de uma conversa de tarefa A2A.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2AMessage {
    /// Papel do emissor (ex: "user", "assistant", "system", etc.).
    pub role: String,

    /// Partes que compõem a mensagem.
    pub parts: Vec<A2APart>,
}

/// Uma parte de uma mensagem A2A.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum A2APart {
    /// Parte textual simples.
    #[serde(rename = "text")]
    Text { text: String },

    /// Parte estruturada com dados JSON arbitrários.
    #[serde(rename = "data")]
    Data { data: serde_json::Value },

    /// Parte representando um arquivo.
    #[serde(rename = "file")]
    File {
        /// URI opcional do arquivo.
        #[serde(skip_serializing_if = "Option::is_none")]
        uri: Option<String>,

        /// Nome opcional do arquivo.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,

        /// Tipo MIME opcional do arquivo.
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
}

/// Artefato produzido por uma tarefa.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2AArtifact {
    /// Nome opcional do artefato.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Partes que compõem o artefato.
    pub parts: Vec<A2APart>,

    /// Índice opcional (para ordenação ou agrupamento).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

/// Corpo da requisição para criação de uma nova tarefa.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    /// ID opcional da tarefa (caso o cliente queira definir).
    #[serde(default)]
    pub id: Option<String>,

    /// Mensagem inicial que dispara a tarefa.
    pub message: A2AMessage,

    /// Metadados adicionais enviados junto com a criação.
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}