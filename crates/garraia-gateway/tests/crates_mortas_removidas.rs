//! #1226: `garraia-tools` e `garraia-runtime` sairam do workspace, e com elas
//! o `runtime_handler` do gateway. Este teste impede que um bloco comentado ou
//! um `use` esquecido volte a apontar para o que nao existe mais.

use std::path::{Path, PathBuf};

fn ler(caminho: &Path) -> String {
    match std::fs::read_to_string(caminho) {
        Ok(s) => s,
        Err(e) => panic!("nao consegui ler {}: {e}", caminho.display()),
    }
}

fn fontes_rs(dir: &Path, saida: &mut Vec<PathBuf>) {
    let entradas = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => panic!("nao consegui listar {}: {e}", dir.display()),
    };
    for entrada in entradas.flatten() {
        let caminho = entrada.path();
        if caminho.is_dir() {
            fontes_rs(&caminho, saida);
        } else if caminho.extension().is_some_and(|x| x == "rs") {
            saida.push(caminho);
        }
    }
}

#[test]
fn workspace_nao_lista_as_crates_mortas() {
    let raiz = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifesto = ler(&raiz.join("Cargo.toml"));
    for morta in ["crates/garraia-tools", "crates/garraia-runtime"] {
        assert!(
            !manifesto.contains(morta),
            "{morta} voltou ao workspace; foi removida no #1226"
        );
    }
}

#[test]
fn gateway_nao_referencia_o_runtime_handler_removido() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut arquivos = Vec::new();
    fontes_rs(&src, &mut arquivos);
    assert!(!arquivos.is_empty(), "nenhum .rs em {}", src.display());
    for arquivo in arquivos {
        let texto = ler(&arquivo);
        for proibido in ["runtime_handler", "garraia_runtime", "garraia_tools"] {
            assert!(
                !texto.contains(proibido),
                "{} ainda cita `{proibido}`, removido no #1226 (inclusive em \
                 comentario: codigo comentado apontando para modulo inexistente \
                 so engana quem le)",
                arquivo.display()
            );
        }
    }
}
