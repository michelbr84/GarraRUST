//! #1543: os textos de remediation mandavam o operador rodar
//! `garraia mcp restart <nome>` — subcomando que a CLI nunca teve. Em
//! producao (0.4.6) o registro de capacidades exibia o MCP filesystem em
//! reconnect e o proximo passo sugerido era um comando inexistente, o que
//! alongou o diagnostico.
//!
//! O caminho real e `POST /admin/api/mcp/{id}/restart`
//! (`admin/routes.rs`, com o botao Restart da aba MCP Servers do console).
//!
//! Este teste tem duas metades que se sustentam: a CLI de fato nao tem o
//! subcomando, e nenhum fonte do gateway volta a cita-lo. Se um dia
//! `garra mcp restart` existir (caminho 1 da #1543), a primeira metade falha
//! primeiro e avisa que os textos podem voltar a mencionar a CLI.

use std::path::{Path, PathBuf};

fn ler(caminho: &Path) -> String {
    match std::fs::read_to_string(caminho) {
        Ok(s) => s,
        Err(e) => panic!("nao consegui ler {}: {e}", caminho.display()),
    }
}

fn raiz() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fontes(dir: &Path, saida: &mut Vec<PathBuf>) {
    let entradas = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => panic!("nao consegui listar {}: {e}", dir.display()),
    };
    for entrada in entradas.flatten() {
        let caminho = entrada.path();
        if caminho.is_dir() {
            fontes(&caminho, saida);
        } else if caminho
            .extension()
            .is_some_and(|x| x == "rs" || x == "html")
        {
            saida.push(caminho);
        }
    }
}

/// A metade que ancora a outra: o `enum McpCommands` da CLI e a verdade sobre
/// quais subcomandos `mcp` existem.
#[test]
fn a_cli_nao_tem_o_subcomando_mcp_restart() {
    let fonte = ler(&raiz().join("crates/garraia-cli/src/main.rs"));
    let inicio = fonte
        .find("enum McpCommands {")
        .expect("enum McpCommands sumiu da CLI — este teste precisa de um novo ponto de ancora");
    let corpo = &fonte[inicio..];
    let fim = corpo
        .find("\n}")
        .expect("nao achei o fim do enum McpCommands");
    let corpo = &corpo[..fim];

    assert!(
        !corpo.contains("Restart"),
        "`garra mcp restart` passou a existir: revise a #1543 e os textos de \
         remediation, que hoje apontam so para POST /admin/api/mcp/<nome>/restart"
    );
}

#[test]
fn nenhum_texto_do_gateway_manda_rodar_mcp_restart() {
    let src = raiz().join("crates/garraia-gateway/src");
    let mut arquivos = Vec::new();
    fontes(&src, &mut arquivos);
    assert!(!arquivos.is_empty(), "varredura nao achou fonte nenhum");

    let mut ofensores = Vec::new();
    for arquivo in &arquivos {
        let conteudo = ler(arquivo);
        for (n, linha) in conteudo.lines().enumerate() {
            if linha.contains("mcp restart") {
                ofensores.push(format!("{}:{}", arquivo.display(), n + 1));
            }
        }
    }

    assert!(
        ofensores.is_empty(),
        "remediation citando `mcp restart`, subcomando que a CLI nao tem (#1543). \
         Use `POST /admin/api/mcp/<nome>/restart`. Linhas: {}",
        ofensores.join(", ")
    );
}
