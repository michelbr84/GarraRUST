use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use axum::{Json, http::StatusCode, response::IntoResponse};

/// Teto de bytes que um leitor do `garraia.log` carrega por requisicao.
///
/// O log cresce sem rotacao (o daemon abre em append desde a #1371, como o
/// `start` em foreground ja abria), entao ler o arquivo inteiro custaria
/// memoria e CPU proporcionais a toda a historia do daemon a cada GET.
pub(crate) const MAX_TAIL_BYTES: u64 = 512 * 1024;

/// Le no maximo os ultimos `max_bytes` de `path`, em UTF-8 lossy.
///
/// - Busca a partir do fim e le com `take(max_bytes)`: o teto vale mesmo que
///   o daemon acrescente linhas entre o `metadata` e a leitura.
/// - Bytes que nao sao UTF-8 viram U+FFFD em vez de derrubar a leitura: o
///   descritor que o daemon herda como stdout/stderr recebe escrita crua (a
///   mensagem de um panic, um filho com o stderr herdado), e nada garante que
///   ela seja UTF-8.
/// - Quando o arquivo passa do teto, a primeira linha lida quase sempre
///   comeca no meio; ela e descartada, e com ela o pedaco de caractere
///   multibyte que o corte possa ter partido.
pub(crate) fn read_log_tail(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let file_len = file.metadata()?.len();
    let truncado = file_len > max_bytes;
    if truncado {
        file.seek(SeekFrom::Start(file_len - max_bytes))?;
    }

    let mut raw = Vec::new();
    file.take(max_bytes).read_to_end(&mut raw)?;
    let mut buf = String::from_utf8_lossy(&raw).into_owned();

    if truncado && let Some(pos) = buf.find('\n') {
        buf.drain(..=pos);
    }
    Ok(buf)
}

/// GET /api/logs
pub async fn get_logs() -> impl IntoResponse {
    let garraia_dir = garraia_config::ConfigLoader::default_config_dir();
    let log_path = garraia_dir.join("garraia.log");

    if !log_path.exists() {
        // Return 200 with a hint — 404 causes the frontend to show a generic "server unavailable" error.
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "logs": format!(
                    "Nenhum arquivo de log encontrado em {}.\n\
                     Para habilitar logs em arquivo, configure RUST_LOG e redirecione a saída:\n\
                     RUST_LOG=garraia=info garraia start 2>&1 | tee ~/.garraia/garraia.log",
                    log_path.display()
                )
            })),
        )
            .into_response();
    }

    let read_result = read_log_tail(&log_path, MAX_TAIL_BYTES).map(|buf| {
        let lines: Vec<&str> = buf.lines().collect();
        let start = lines.len().saturating_sub(1000);
        lines[start..].join("\n")
    });

    match read_result {
        Ok(recent_lines) => (
            StatusCode::OK,
            Json(serde_json::json!({ "logs": recent_lines })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cauda_de_arquivo_abaixo_do_teto_volta_inteira() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("garraia.log");
        std::fs::write(&path, "um\ndois\ntres").expect("semeia");
        assert_eq!(read_log_tail(&path, 1024).expect("le"), "um\ndois\ntres");
    }

    /// O corte cai no meio do `c` cedilhado (2 bytes) da primeira linha lida:
    /// a linha partida some inteira, e com ela o U+FFFD do caractere cortado.
    #[test]
    fn cauda_acima_do_teto_descarta_a_linha_partida_e_o_caractere_cortado() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("garraia.log");
        std::fs::write(&path, "a\u{e7}\n".repeat(100)).expect("semeia");
        // 100 linhas de 4 bytes; o teto de 398 comeca no byte 2, o segundo do `c`.
        let cauda = read_log_tail(&path, 398).expect("le");
        assert_eq!(cauda, "a\u{e7}\n".repeat(99));
        assert!(!cauda.contains('\u{FFFD}'));
    }
}
