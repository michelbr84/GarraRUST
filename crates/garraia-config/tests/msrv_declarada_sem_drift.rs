//! #1750: todo espelho da MSRV bate com o `rust-version` do workspace.
//!
//! O #1731 subiu o `rust-version` para 1.97 e atualizou o manifesto, o
//! `ci.yml` e parte da documentacao, mas README, CONTRIBUTING,
//! `scripts/setup-toolchain.sh` (que passou a fazer `rustup override set 1.96`
//! num workspace que exige 1.97 — build quebrado para quem seguia o
//! CONTRIBUTING), `scripts/build-installer.ps1`, a wiki publicada e docs mais
//! antigas ficaram para tras. Estes testes leem os arquivos do repositorio em
//! tempo de execucao (nunca `include_str!`) e travam cada espelho: a fonte de
//! verdade e o `Cargo.toml` da raiz, e qualquer bump futuro fica vermelho ate
//! todos os espelhos subirem juntos.

use std::path::{Path, PathBuf};

/// Onde cada espelho fala da MSRV. `{msrv}` e substituido pelo valor do
/// `Cargo.toml`. Trechos curtos de proposito: reformular a frase em volta nao
/// quebra o teste, mudar o numero sem mudar aqui quebra.
const ESPELHOS: &[(&str, &[&str])] = &[
    (
        "crates/garraia-glob/Cargo.toml",
        &[r#"rust-version = "{msrv}""#],
    ),
    (
        "README.md",
        &["rust-{msrv}%2B-orange", "Requires Rust {msrv}+"],
    ),
    (
        "README.pt-BR.md",
        &["rust-{msrv}%2B-orange", "Requer Rust {msrv}+"],
    ),
    (
        "CONTRIBUTING.md",
        &[
            "| Rust | {msrv} |",
            "Rust {msrv}+",
            "rustup override set {msrv}",
        ],
    ),
    ("scripts/setup-toolchain.sh", &[r#"MSRV="{msrv}""#]),
    ("scripts/build-installer.ps1", &["Rust {msrv}+"]),
    (
        ".github/workflows/ci.yml",
        &[
            "name: MSRV check ({msrv})",
            "dtolnay/rust-toolchain@{msrv}",
            "cargo +{msrv} check",
        ],
    ),
    ("AGENTS.md", &[r#"rust-version = "{msrv}""#]),
    ("docs/installation.md", &["Rust {msrv}+"]),
    ("docs/contributing.md", &["Rust {msrv}+"]),
    ("docs/src/getting-started.md", &["| Rust | {msrv} |"]),
    ("docs/src/guides/create-plugin.md", &["Rust {msrv}+"]),
    (
        "wiki/Instalacao-e-Primeiros-Passos.md",
        &["requer Rust {msrv}+"],
    ),
    ("wiki/Contribuir-Roadmap-e-FAQ.md", &["Rust {msrv}+"]),
    (
        "website/content/en/instalacao.md",
        &["Requires Rust {msrv}+"],
    ),
    (".claude/superpowers-config.md", &["rust-version {msrv}"]),
];

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

/// O `rust-version` do `[workspace.package]` da raiz — a fonte de verdade.
fn msrv_do_workspace() -> String {
    let manifesto = ler("Cargo.toml");
    let linha = manifesto
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("rust-version"))
        .expect("Cargo.toml da raiz declara rust-version");
    let valor = linha
        .split_once('=')
        .map(|(_, v)| v.trim().trim_matches('"'))
        .expect("rust-version = \"X.Y\"");
    valor.to_string()
}

#[test]
fn msrv_do_workspace_tem_a_forma_x_y() {
    let msrv = msrv_do_workspace();
    let partes: Vec<&str> = msrv.split('.').collect();
    assert_eq!(
        partes.len(),
        2,
        "rust-version deveria ser X.Y, veio {msrv:?}"
    );
    assert!(
        partes
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())),
        "rust-version deveria ser X.Y numerico, veio {msrv:?}"
    );
}

#[test]
fn todo_espelho_da_msrv_bate_com_o_cargo_toml() {
    let msrv = msrv_do_workspace();
    let mut faltando = Vec::new();
    for (arquivo, trechos) in ESPELHOS {
        let conteudo = ler(arquivo);
        for trecho in trechos.iter() {
            let esperado = trecho.replace("{msrv}", &msrv);
            if !conteudo.contains(&esperado) {
                faltando.push(format!("{arquivo}: falta {esperado:?}"));
            }
        }
    }
    assert!(
        faltando.is_empty(),
        "espelhos da MSRV {msrv} desalinhados (suba-os junto com o Cargo.toml):\n  {}",
        faltando.join("\n  ")
    );
}

/// O caso do #1750: o trecho certo com o numero errado. `1.9x` cobre toda a
/// serie de pisos ja vistos (1.92 a 1.97) e os proximos.
#[test]
fn nenhum_espelho_cita_outra_versao_no_mesmo_lugar() {
    let msrv = msrv_do_workspace();
    let mut sobras = Vec::new();
    for (arquivo, trechos) in ESPELHOS {
        let conteudo = ler(arquivo);
        for trecho in trechos.iter() {
            for x in 0..=9 {
                let outra = format!("1.9{x}");
                if outra == msrv {
                    continue;
                }
                let errado = trecho.replace("{msrv}", &outra);
                if conteudo.contains(&errado) {
                    sobras.push(format!("{arquivo}: ainda diz {errado:?}"));
                }
            }
        }
    }
    assert!(
        sobras.is_empty(),
        "espelhos com versao antiga (MSRV atual {msrv}):\n  {}",
        sobras.join("\n  ")
    );
}
