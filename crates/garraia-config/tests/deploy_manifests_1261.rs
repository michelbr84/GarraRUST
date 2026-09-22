//! #1261: todo manifesto de deploy que roda a imagem publicada tem de
//! entregar `GARRAIA_GATEWAY_API_KEY`.
//!
//! A imagem liga em `0.0.0.0` (`CMD ["start", "--host", "0.0.0.0"]`) e o
//! `garraia start` recusa esse bind sem credencial de gateway (exit 78). Um
//! manifesto que esquece a env sobe um container em loop de reinicio no
//! primeiro upgrade — foi o que o chart Helm e o modulo Terraform/ECS fariam
//! na v0.4.5 se ninguem os tivesse tocado. Estes testes leem os arquivos do
//! repositorio em tempo de execucao (nunca `include_str!`, que a imagem
//! Docker nao copia) e travam cada um.

use std::path::{Path, PathBuf};

const CHAVE: &str = "GARRAIA_GATEWAY_API_KEY";

fn raiz() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("raiz do repositorio")
}

fn ler(relativo: &str) -> String {
    let caminho = raiz().join(relativo);
    std::fs::read_to_string(&caminho)
        .unwrap_or_else(|e| panic!("nao consegui ler {}: {e}", caminho.display()))
}

/// Pre-condicao: se a imagem deixar de ligar em `0.0.0.0`, os testes abaixo
/// perdem o motivo e precisam ser revistos, nao apagados em silencio.
#[test]
fn a_imagem_liga_em_bind_exposto() {
    let dockerfile = ler("Dockerfile");
    assert!(
        dockerfile.contains(r#"CMD ["start", "--host", "0.0.0.0"]"#),
        "o CMD do Dockerfile mudou: revise estes testes do #1261"
    );
}

#[test]
fn helm_injeta_a_chave_por_secret() {
    let deployment = ler("deploy/helm/garraia/templates/deployment.yaml");
    let bloco = deployment
        .split(&format!("- name: {CHAVE}"))
        .nth(1)
        .unwrap_or_else(|| panic!("deployment.yaml nao define {CHAVE} no env do container"));
    let bloco: String = bloco.lines().take(5).collect::<Vec<_>>().join("\n");
    assert!(
        bloco.contains("secretKeyRef:"),
        "{CHAVE} tem de vir de um Secret, nunca de valor literal:\n{bloco}"
    );

    let secret = ler("deploy/helm/garraia/templates/gateway-key-secret.yaml");
    assert!(
        secret.contains(&format!("{CHAVE}:")),
        "o Secret gerado nao carrega {CHAVE}"
    );
    assert!(
        secret.contains("lookup"),
        "a chave gerada tem de sobreviver ao upgrade (lookup do Secret existente)"
    );

    let values = ler("deploy/helm/garraia/values.yaml");
    assert!(
        values.contains("gatewayApiKey:"),
        "values.yaml nao documenta gatewayApiKey"
    );
}

#[test]
fn terraform_exige_a_chave_e_a_injeta() {
    let variaveis = ler("deploy/terraform/variables.tf");
    let bloco = variaveis
        .split(r#"variable "gateway_api_key_secret_arn""#)
        .nth(1)
        .expect("variables.tf nao declara gateway_api_key_secret_arn");
    let bloco = bloco.split("\nvariable ").next().unwrap_or(bloco);
    assert!(
        !bloco.contains("default"),
        "gateway_api_key_secret_arn tem de ser obrigatoria (sem default)"
    );

    let main = ler("deploy/terraform/main.tf");
    assert!(
        main.contains(&format!(
            r#"{{ name = "{CHAVE}", valueFrom = var.gateway_api_key_secret_arn }}"#
        )),
        "main.tf nao injeta {CHAVE} a partir de gateway_api_key_secret_arn"
    );
    assert!(
        main.contains("secrets = local.container_secrets"),
        "o container tem de receber local.container_secrets, nao so var.secrets"
    );
}

/// Todo `docker-compose*.yml` da raiz que roda o gateway (`build:` ou a imagem
/// publicada) repassa a env — `env_file` sozinho nao basta: o `.env` pode nao
/// existir, e o `environment:` deixa a dependencia visivel no manifesto.
#[test]
fn todo_compose_do_gateway_repassa_a_chave() {
    let mut vistos = 0;
    for entrada in std::fs::read_dir(raiz()).expect("ler a raiz") {
        let caminho = entrada.expect("entrada").path();
        let nome = caminho.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(nome.starts_with("docker-compose") && nome.ends_with(".yml")) {
            continue;
        }
        let conteudo = std::fs::read_to_string(&caminho).expect("ler compose");
        let roda_o_gateway = conteudo.contains("garraia:")
            && (conteudo.contains("build:") || conteudo.contains("ghcr.io/michelbr84/garraia"));
        if !roda_o_gateway {
            continue;
        }
        vistos += 1;
        assert!(
            conteudo.contains(&format!("- {CHAVE}=${{{CHAVE}:-}}")),
            "{nome} roda o gateway sem repassar {CHAVE}"
        );
    }
    assert!(
        vistos >= 3,
        "esperava ao menos 3 compose do gateway, vi {vistos}"
    );
}

/// O `.env.example` traz a linha, e VAZIA: um placeholder seria uma chave que
/// todo mundo conhece, e o gate aceitaria o placeholder copiado sem troca.
#[test]
fn env_example_traz_a_linha_vazia() {
    let exemplo = ler(".env.example");
    let linha = exemplo
        .lines()
        .find(|l| l.starts_with(&format!("{CHAVE}=")))
        .unwrap_or_else(|| panic!(".env.example nao traz {CHAVE}="));
    assert_eq!(
        linha.trim(),
        format!("{CHAVE}="),
        "o placeholder tem de ser vazio"
    );
}
