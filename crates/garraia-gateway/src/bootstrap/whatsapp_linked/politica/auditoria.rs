//! Trilha local de mudancas na politica de acesso (#1414).
//!
//! Um JSONL em `<data_dir>/audit/whatsapp-access.jsonl`, `0600`, uma linha
//! por mutacao aplicada: quando (UTC, `Z`), quem (usuario do SO ou admin do
//! console), por onde (`cli` | `admin_api` | `web`), o que (`acao`), em quem
//! (`…1234`, nunca a identidade) e o **resumo** da politica antes e depois.
//! Nunca chave de API, material de sessao nem texto de mensagem — este
//! arquivo so conhece a politica.
//!
//! Rotacao por tamanho: passou de `max_bytes`, o arquivo vira `.1` (um so) e
//! o novo comeca vazio — o disco nunca cresce sem limite. Falhar ao gravar e
//! `Err` para quem chama decidir; nunca panico, nunca "gravou" em silencio.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::LinkedSettings;
use super::mutacao::mascarar;

/// Caminho relativo ao diretorio de dados.
pub const ARQUIVO: &str = "audit/whatsapp-access.jsonl";
/// Teto default de rotacao: 5 MiB.
pub const MAX_BYTES_DEFAULT: u64 = 5 * 1024 * 1024;

/// Uma linha do audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evento {
    /// UTC ISO 8601 com `Z`.
    pub ts: String,
    /// `cli` | `admin_api` | `web`.
    pub origem: String,
    /// Usuario do SO (CLI) ou username do admin (console/API).
    pub ator: String,
    /// `open` | `restricted` | `default` | `level` | `write` | `block` |
    /// `unblock` | `groups` | `group-default` | `group` | `reset` | ...
    pub acao: String,
    /// `…1234`, ou `None` quando a acao nao mira uma identidade.
    pub alvo: Option<String>,
    pub antes: serde_json::Value,
    pub depois: serde_json::Value,
}

impl Evento {
    /// Monta o evento **ja mascarado**: `alvo` vira `…1234` e os dois estados
    /// viram o resumo de [`resumo`].
    pub fn novo(
        origem: &str,
        ator: &str,
        acao: &str,
        alvo: Option<&str>,
        antes: &LinkedSettings,
        depois: &LinkedSettings,
    ) -> Self {
        Self {
            ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            origem: origem.to_string(),
            ator: ator.to_string(),
            acao: acao.to_string(),
            alvo: alvo.map(mascarar),
            antes: resumo(antes),
            depois: resumo(depois),
        }
    }
}

/// O resumo de uma politica para o audit: contagens, admissao, default,
/// grupos e cada usuario declarado por `…1234` — nunca a identidade.
pub fn resumo(settings: &LinkedSettings) -> serde_json::Value {
    let mut chaves: Vec<String> = settings.chaves_declaradas().into_iter().collect();
    chaves.sort();
    let users: Vec<serde_json::Value> = chaves
        .iter()
        .filter_map(|chave| {
            settings.entrada_de(chave).map(|e| {
                serde_json::json!({
                    "alvo": mascarar(chave),
                    "level": e.alcance.nivel.as_str(),
                    "write": e.alcance.write,
                    "owner": e.dono,
                    "blocked": e.bloqueado,
                })
            })
        })
        .collect();
    serde_json::json!({
        "enabled": settings.enabled,
        "admission": settings.access.admission.as_str(),
        "default": { "level": settings.access.default.nivel.as_str(), "write": settings.access.default.write },
        "groups": {
            "enabled": settings.responde_em_grupo(),
            "default": { "level": settings.access.groups.default.nivel.as_str(), "write": settings.access.groups.default.write },
            "declared": settings.access.groups.por_grupo.len(),
        },
        "allow": settings.allow.len(),
        "owners": settings.owners.len(),
        "users": users,
    })
}

/// Acrescenta uma linha, rotacionando antes se o arquivo passou de
/// `max_bytes`. Devolve o caminho gravado.
pub fn registrar(data_dir: &Path, evento: &Evento, max_bytes: u64) -> io::Result<PathBuf> {
    let caminho = data_dir.join(ARQUIVO);
    let dir = caminho
        .parent()
        .ok_or_else(|| io::Error::other("caminho do audit sem diretorio"))?;
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Best-effort: o que importa e o 0600 do arquivo, abaixo.
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    if let Ok(meta) = std::fs::metadata(&caminho)
        && meta.len() > max_bytes
    {
        std::fs::rename(&caminho, dir.join("whatsapp-access.jsonl.1"))?;
    }
    let mut opcoes = std::fs::OpenOptions::new();
    opcoes.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opcoes.mode(0o600);
    }
    let mut arquivo = opcoes.open(&caminho)?;
    let linha = serde_json::to_string(evento).map_err(io::Error::other)?;
    arquivo.write_all(linha.as_bytes())?;
    arquivo.write_all(b"\n")?;
    arquivo.flush()?;
    Ok(caminho)
}

/// As ultimas `limite` linhas, **mais recente primeiro**. Sem arquivo, vazio.
/// Linha ilegivel e pulada, nao derruba a leitura.
pub fn ler(data_dir: &Path, limite: usize) -> io::Result<Vec<Evento>> {
    let caminho = data_dir.join(ARQUIVO);
    let conteudo = match std::fs::read_to_string(&caminho) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let todos: Vec<Evento> = conteudo
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let inicio = todos.len().saturating_sub(limite);
    let mut recentes: Vec<Evento> = todos[inicio..].to_vec();
    recentes.reverse();
    Ok(recentes)
}
