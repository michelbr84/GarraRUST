//! Classes de capacidade (#1385): o que uma ferramenta **faz**, declarado
//! uma vez, para a politica poder falar de classes em vez de nomes.
//!
//! Ate aqui `allowed`/`denied` eram listas de nomes (e prefixos de servidor
//! MCP). Nome nao escala: uma ferramenta MCP nova de leitura precisava de uma
//! entrada manual para ser vista num modo restrito, e uma de escrita passava
//! se alguem tivesse liberado o servidor inteiro. A classe e o que o
//! `write on|off` do WhatsApp (#1392) compila: "tirar mutacao de arquivo" e
//! `filesystem.write`, seja a tool nativa `file_write`, seja
//! `filesystem__write_file`, seja um servidor MCP que se declare destrutivo.
//!
//! Regras:
//!
//! - Ferramenta **nativa** e classificada por nome numa tabela fechada
//!   ([`NATIVAS`]). Nome fora da tabela nao tem classe.
//! - Ferramenta **MCP** e classificada pela operacao de filesystem conhecida
//!   (mesma lista do confinamento, #1482) ou, fora dela, pelas anotacoes do
//!   servidor (`readOnlyHint`/`destructiveHint`): leitura declarada vira
//!   [`Capacidade::McpRead`], escrita ou destruicao declarada vira
//!   [`Capacidade::McpWrite`]. Sem anotacao, sem classe.
//! - **Sem classe e fail-closed** num modo que restringe por classe: o que o
//!   runtime nao sabe o que faz, nao roda so porque ninguem o proibiu.
//! - Nome continua valendo (`allowed`/`denied` por nome), para precisao e
//!   compatibilidade: os dois criterios somam, e `denied` de qualquer forma
//!   vence.

use serde::{Deserialize, Serialize};

/// Uma classe do que a ferramenta faz. O nome serializado e o do
/// vocabulario da #1385 (`filesystem.read`, `process.execute`…).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Capacidade {
    #[serde(rename = "filesystem.read")]
    FilesystemRead,
    #[serde(rename = "filesystem.write")]
    FilesystemWrite,
    #[serde(rename = "process.execute")]
    ProcessExecute,
    #[serde(rename = "network.read")]
    NetworkRead,
    #[serde(rename = "message.send")]
    MessageSend,
    #[serde(rename = "device.read")]
    DeviceRead,
    #[serde(rename = "device.execute")]
    DeviceExecute,
    #[serde(rename = "memory.read")]
    MemoryRead,
    #[serde(rename = "memory.write")]
    MemoryWrite,
    #[serde(rename = "runtime.inspect")]
    RuntimeInspect,
    #[serde(rename = "scheduling")]
    Scheduling,
    #[serde(rename = "mcp.read")]
    McpRead,
    #[serde(rename = "mcp.write")]
    McpWrite,
}

impl Capacidade {
    /// Todas, na ordem do vocabulario.
    pub const TODAS: &'static [Capacidade] = &[
        Self::FilesystemRead,
        Self::FilesystemWrite,
        Self::ProcessExecute,
        Self::NetworkRead,
        Self::MessageSend,
        Self::DeviceRead,
        Self::DeviceExecute,
        Self::MemoryRead,
        Self::MemoryWrite,
        Self::RuntimeInspect,
        Self::Scheduling,
        Self::McpRead,
        Self::McpWrite,
    ];

    /// As classes que so **leem**: o que um nivel somente-leitura libera.
    pub const LEITURA: &'static [Capacidade] = &[
        Self::FilesystemRead,
        Self::NetworkRead,
        Self::DeviceRead,
        Self::MemoryRead,
        Self::RuntimeInspect,
        Self::McpRead,
    ];

    /// O que o `write on|off` liga e desliga (#1392): mutacao de arquivo,
    /// nativa ou por MCP. Shell, dispositivo, mensagem e agenda ficam de
    /// fora de proposito — sao controles independentes.
    pub const ESCRITA_DE_ARQUIVO: &'static [Capacidade] = &[Self::FilesystemWrite, Self::McpWrite];

    /// O nome serializado (`filesystem.read`…).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FilesystemRead => "filesystem.read",
            Self::FilesystemWrite => "filesystem.write",
            Self::ProcessExecute => "process.execute",
            Self::NetworkRead => "network.read",
            Self::MessageSend => "message.send",
            Self::DeviceRead => "device.read",
            Self::DeviceExecute => "device.execute",
            Self::MemoryRead => "memory.read",
            Self::MemoryWrite => "memory.write",
            Self::RuntimeInspect => "runtime.inspect",
            Self::Scheduling => "scheduling",
            Self::McpRead => "mcp.read",
            Self::McpWrite => "mcp.write",
        }
    }

    /// O inverso de [`Self::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        Self::TODAS.iter().copied().find(|c| c.as_str() == s.trim())
    }

    /// Muda algo fora da conversa (arquivo, processo, dispositivo, mensagem,
    /// agenda, memoria). E o que "somente leitura" nega.
    pub fn e_mutante(self) -> bool {
        !Self::LEITURA.contains(&self)
    }
}

impl std::fmt::Display for Capacidade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A tabela fechada das ferramentas nativas. Uma tool nova entra aqui ou
/// fica sem classe — e sem classe nao passa em modo restrito por classe.
pub const NATIVAS: &[(&str, &[Capacidade])] = &[
    ("file_read", &[Capacidade::FilesystemRead]),
    ("list_dir", &[Capacidade::FilesystemRead]),
    ("repo_search", &[Capacidade::FilesystemRead]),
    ("git_diff", &[Capacidade::FilesystemRead]),
    ("code_review", &[Capacidade::FilesystemRead]),
    ("file_write", &[Capacidade::FilesystemWrite]),
    ("bash", &[Capacidade::ProcessExecute]),
    ("run_tests", &[Capacidade::ProcessExecute]),
    ("web_search", &[Capacidade::NetworkRead]),
    ("web_fetch", &[Capacidade::NetworkRead]),
    ("telegram_send", &[Capacidade::MessageSend]),
    ("device_list", &[Capacidade::DeviceRead]),
    ("device_read", &[Capacidade::DeviceRead]),
    ("device_execute", &[Capacidade::DeviceExecute]),
    ("garra_status", &[Capacidade::RuntimeInspect]),
    ("schedule_heartbeat", &[Capacidade::Scheduling]),
    ("schedule_recurring", &[Capacidade::Scheduling]),
];

/// As classes de uma ferramenta nativa, por nome. Vazio para nome
/// desconhecido — inclusive `tool_program`, que e um orquestrador e cujos
/// passos passam, cada um, pelo proprio portao.
pub fn capacidades_nativas(nome: &str) -> &'static [Capacidade] {
    NATIVAS
        .iter()
        .find(|(n, _)| *n == nome)
        .map(|(_, c)| *c)
        .unwrap_or(&[])
}

/// Operacoes de **leitura** do `@modelcontextprotocol/server-filesystem`.
pub const MCP_FILESYSTEM_LEITURA: &[&str] = &[
    "read_file",
    "read_text_file",
    "read_media_file",
    "read_multiple_files",
    "list_directory",
    "list_directory_with_sizes",
    "directory_tree",
    "search_files",
    "get_file_info",
    "list_allowed_directories",
];

/// Operacoes de **escrita** do mesmo servidor.
pub const MCP_FILESYSTEM_ESCRITA: &[&str] =
    &["write_file", "edit_file", "create_directory", "move_file"];

/// As classes de uma ferramenta MCP: pela operacao de filesystem conhecida
/// (por nome, em qualquer servidor), senao pelas anotacoes que o servidor
/// declarou. Anotacoes contraditorias (`readOnly` e `destructive`) valem
/// como nenhuma.
pub fn capacidades_da_operacao_mcp(
    operacao: &str,
    read_only_hint: Option<bool>,
    destructive_hint: Option<bool>,
) -> &'static [Capacidade] {
    if MCP_FILESYSTEM_LEITURA.contains(&operacao) {
        return &[Capacidade::FilesystemRead];
    }
    if MCP_FILESYSTEM_ESCRITA.contains(&operacao) {
        return &[Capacidade::FilesystemWrite];
    }
    match (read_only_hint, destructive_hint) {
        (Some(true), Some(true)) => &[],
        (Some(true), _) => &[Capacidade::McpRead],
        (Some(false), _) | (None, Some(true)) => &[Capacidade::McpWrite],
        (None, _) => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Toda tool nativa que o gateway registra tem classe — e a lista e a
    /// dos `impl Tool` das duas crates (agents + gateway), sem `tool_program`.
    #[test]
    fn toda_tool_nativa_conhecida_tem_classe() {
        for nome in [
            "bash",
            "code_review",
            "device_execute",
            "device_list",
            "device_read",
            "file_read",
            "file_write",
            "garra_status",
            "git_diff",
            "list_dir",
            "repo_search",
            "run_tests",
            "telegram_send",
            "web_fetch",
            "web_search",
            "schedule_heartbeat",
            "schedule_recurring",
        ] {
            assert!(!capacidades_nativas(nome).is_empty(), "{nome} sem classe");
        }
        assert!(capacidades_nativas("tool_program").is_empty());
        assert!(capacidades_nativas("inventada").is_empty());
    }

    #[test]
    fn o_vocabulario_serializado_e_o_da_issue() {
        for c in Capacidade::TODAS {
            let json = serde_json::to_string(c).expect("json");
            assert_eq!(json, format!("\"{}\"", c.as_str()));
            assert_eq!(Capacidade::parse(c.as_str()), Some(*c));
            assert_eq!(
                serde_json::from_str::<Capacidade>(&json).expect("volta"),
                *c
            );
        }
        assert_eq!(Capacidade::FilesystemRead.as_str(), "filesystem.read");
        assert_eq!(Capacidade::ProcessExecute.as_str(), "process.execute");
        assert!(Capacidade::parse("x").is_none());
    }

    #[test]
    fn leitura_e_escrita_de_arquivo_sao_disjuntas_e_mutante_e_o_resto() {
        for c in Capacidade::LEITURA {
            assert!(!c.e_mutante(), "{c}");
            assert!(!Capacidade::ESCRITA_DE_ARQUIVO.contains(c), "{c}");
        }
        for c in Capacidade::ESCRITA_DE_ARQUIVO {
            assert!(c.e_mutante(), "{c}");
        }
        assert!(Capacidade::ProcessExecute.e_mutante());
        assert!(Capacidade::MessageSend.e_mutante());
        assert!(!Capacidade::RuntimeInspect.e_mutante());
    }

    /// MCP: operacao de filesystem por nome em qualquer servidor; fora dela,
    /// as anotacoes; sem anotacao, sem classe (fail-closed em modo restrito).
    #[test]
    fn operacao_mcp_e_classificada_por_nome_e_depois_por_anotacao() {
        assert_eq!(
            capacidades_da_operacao_mcp("read_text_file", None, None),
            &[Capacidade::FilesystemRead]
        );
        assert_eq!(
            capacidades_da_operacao_mcp("list_allowed_directories", None, None),
            &[Capacidade::FilesystemRead]
        );
        assert_eq!(
            capacidades_da_operacao_mcp("write_file", Some(true), None),
            &[Capacidade::FilesystemWrite],
            "o nome vence a anotacao mentirosa"
        );
        assert_eq!(
            capacidades_da_operacao_mcp("search_issues", Some(true), None),
            &[Capacidade::McpRead]
        );
        assert_eq!(
            capacidades_da_operacao_mcp("search_issues", Some(true), Some(false)),
            &[Capacidade::McpRead]
        );
        assert_eq!(
            capacidades_da_operacao_mcp("create_issue", Some(false), None),
            &[Capacidade::McpWrite]
        );
        assert_eq!(
            capacidades_da_operacao_mcp("drop_table", None, Some(true)),
            &[Capacidade::McpWrite]
        );
        assert!(capacidades_da_operacao_mcp("misterio", None, None).is_empty());
        assert!(capacidades_da_operacao_mcp("misterio", None, Some(false)).is_empty());
        assert!(capacidades_da_operacao_mcp("contraditoria", Some(true), Some(true)).is_empty());
    }
}
