//! #1858: append JSONL do registro de auditoria sob `never-confirm` (`--yolo`).
//!
//! Fonte única do formato de arquivo para as tools que dispensam o pedido de
//! confirmação (`bash`, `run_tests`, `device_execute`). Cada chamada acrescenta
//! uma linha JSON em `<data_dir>/audit/never-confirm.jsonl`. Best-effort: falha
//! de disco vira WARN e não derruba a execução — a linha WARN da própria tool
//! é a segunda camada. Quem chama decide os campos e é responsável por não
//! colocar segredo neles (o `bash` passa o comando pelo redator de log; o
//! `device_execute` não grava `args`).

use std::path::Path;

/// Acrescenta `registro` como uma linha JSONL em `caminho`, criando os
/// diretórios na primeira escrita.
pub(crate) async fn anexar(caminho: &Path, registro: &serde_json::Value) {
    if let Some(pai) = caminho.parent()
        && let Err(e) = tokio::fs::create_dir_all(pai).await
    {
        tracing::warn!(
            path = ?caminho,
            erro = %e,
            "auditoria_yolo: nao consegui criar o diretorio do arquivo de auditoria"
        );
        return;
    }
    match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(caminho)
        .await
    {
        Ok(mut arquivo) => {
            use tokio::io::AsyncWriteExt as _;
            // `tokio::fs::File` entrega o buffer a uma task de bloqueio:
            // `write_all` volta antes de o dado chegar ao arquivo, e sem o
            // `flush` o registro podia aterrissar depois do retorno (ou fora
            // de ordem com o próximo append). O `flush` espera a escrita.
            let resultado = async {
                arquivo
                    .write_all(format!("{registro}\n").as_bytes())
                    .await?;
                arquivo.flush().await
            }
            .await;
            if let Err(e) = resultado {
                tracing::warn!(path = ?caminho, erro = %e, "auditoria_yolo: falha ao escrever o registro");
            }
        }
        Err(e) => tracing::warn!(
            path = ?caminho,
            erro = %e,
            "auditoria_yolo: falha ao abrir o arquivo de auditoria"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cria_diretorio_e_acumula_linhas() {
        let dir = tempfile::tempdir().expect("tempdir");
        let arquivo = dir.path().join("audit").join("never-confirm.jsonl");
        anexar(&arquivo, &serde_json::json!({"n": 1})).await;
        anexar(&arquivo, &serde_json::json!({"n": 2})).await;
        let conteudo = std::fs::read_to_string(&arquivo).expect("arquivo criado");
        let linhas: Vec<serde_json::Value> = conteudo
            .lines()
            .map(|l| serde_json::from_str(l).expect("linha JSON"))
            .collect();
        assert_eq!(
            linhas,
            vec![serde_json::json!({"n": 1}), serde_json::json!({"n": 2})]
        );
    }
}
