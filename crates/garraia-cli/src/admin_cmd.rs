//! `garra admin ...` — apoio ao painel admin a partir do host (#1122).
//!
//! A recuperacao de senha do painel e assistida pelo CLI de proposito: o
//! codigo de uso unico e gerado pelo gateway e entregue **fora do HTTP**, num
//! arquivo `0600` dentro do data dir. Quem consegue ler o codigo e quem tem
//! shell na maquina — exatamente quem pode rodar este binario.

use std::path::PathBuf;

use garraia_config::AppConfig;
use garraia_gateway::admin::recovery::RECOVERY_CODE_FILE;

/// Tamanho minimo da senha, igual ao do painel (`admin/users.rs`).
const MIN_PASSWORD_LEN: usize = 8;

/// Caminho do arquivo de mao dupla. Mesma fonte do gateway: a constante vem
/// de `garraia_gateway` para que as duas pontas nao divirjam.
fn handoff_path(config: &AppConfig) -> PathBuf {
    config.resolved_data_dir().join(RECOVERY_CODE_FILE)
}

fn base_url(config: &AppConfig) -> String {
    format!("http://{}:{}", config.gateway.host, config.gateway.port)
}

/// `garra admin recovery start --username <u>`
///
/// Pede o codigo ao gateway e, quando roda no mesmo host, le o arquivo e
/// mostra o codigo. Numa maquina remota o arquivo nao existe: ai o comando
/// diz onde ele esta, em vez de fingir que nada aconteceu.
pub async fn run_recovery_start(config: &AppConfig, username: &str) -> anyhow::Result<i32> {
    let url = format!("{}/admin/api/recovery/start", base_url(config));
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({ "username": username }))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nao consegui falar com o gateway em {url}: {e}");
            eprintln!("o gateway precisa estar rodando para gerar o codigo.");
            return Ok(1);
        }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        eprintln!("muitas tentativas. Aguarde um minuto e repita.");
        return Ok(1);
    }
    if !status.is_success() {
        eprintln!("o gateway respondeu {status}. Nada foi gerado.");
        return Ok(1);
    }

    println!("Pedido registrado. O codico vale 10 minutos e serve uma unica vez.");
    match ler_codigo(&handoff_path(config)) {
        Some(code) => {
            println!();
            println!("Codigo: {code}");
            println!();
            println!("Depois: garra admin recovery complete --username <usuario> --code <codigo>");
            Ok(0)
        }
        None => {
            // Gateway remoto (ou data dir diferente): o operador le no host.
            println!(
                "Este comando nao achou o arquivo do codigo — o gateway deve estar em \
                 outro host. Leia-o la:"
            );
            println!("  {}", handoff_path(config).display());
            Ok(0)
        }
    }
}

/// `garra admin recovery complete --username <u> --code <c> [--new-password <p>]`
pub async fn run_recovery_complete(
    config: &AppConfig,
    username: &str,
    code: &str,
    new_password: Option<String>,
) -> anyhow::Result<i32> {
    // `--new-password` aparece em `ps` e no historico do shell; o prompt e o
    // caminho preferido e por isso e o padrao quando a flag nao vem.
    let password = match new_password {
        Some(p) => p,
        None => dialoguer::Password::new()
            .with_prompt("Nova senha (minimo 8 caracteres)")
            .interact()?,
    };

    if password.len() < MIN_PASSWORD_LEN {
        eprintln!("a senha precisa de pelo menos {MIN_PASSWORD_LEN} caracteres.");
        return Ok(2);
    }

    let url = format!("{}/admin/api/recovery/complete", base_url(config));
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({
            "username": username,
            "code": code,
            "new_password": password,
        }))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nao consegui falar com o gateway em {url}: {e}");
            return Ok(1);
        }
    };

    match resp.status() {
        reqwest::StatusCode::OK => {
            println!("Senha redefinida. Todas as sessoes antigas foram revogadas.");
            Ok(0)
        }
        reqwest::StatusCode::UNAUTHORIZED => {
            eprintln!("codigo invalido, expirado ou ja usado. Gere outro com `start`.");
            Ok(1)
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            eprintln!("muitas tentativas. Aguarde um minuto e repita.");
            Ok(1)
        }
        other => {
            eprintln!("o gateway respondeu {other}. A senha nao mudou.");
            Ok(1)
        }
    }
}

/// Le o `code=` do arquivo escrito pelo gateway. `None` quando o arquivo nao
/// existe, nao e legivel ou nao tem a linha — nunca quando o codigo e vazio.
fn ler_codigo(path: &std::path::Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    contents.lines().find_map(|line| {
        let value = line.strip_prefix("code=")?;
        (!value.trim().is_empty()).then(|| value.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_codigo_do_arquivo_de_mao_dupla() {
        let dir = std::env::temp_dir().join(format!("garra-cli-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join(RECOVERY_CODE_FILE);
        std::fs::write(&path, "username=admin\ncode=abc123\n").expect("write");

        assert_eq!(ler_codigo(&path).as_deref(), Some("abc123"));

        std::fs::write(&path, "username=admin\n").expect("write");
        assert_eq!(ler_codigo(&path), None);

        assert_eq!(ler_codigo(&dir.join("nao-existe")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
