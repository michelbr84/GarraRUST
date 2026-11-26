//! #2021: todo teste que sobe o `GatewayServer` de verdade desvia o config dir.
//!
//! `GatewayServer::run()` abre o `admin.db`, resolve o `master.key` e grava o
//! `sessions.db` sob o `<config_dir>`: o de `GARRAIA_CONFIG_DIR` ou, sem ele,
//! o XDG de quem roda `cargo test` (`~/.config/garraia`). Um alvo de teste que
//! sobe o servidor sem desviar esse diretorio trabalha sobre os dados reais
//! do operador. Com um `master.key` de tamanho errado e o `admin.db` real
//! vazio, por exemplo, o boot troca a chave do operador por outra.
//!
//! O teste varre os `tests/` de todas as crates. Quem chama
//! `GatewayServer::new` precisa chamar `config_dir_descartavel::isolar()` ou
//! apontar `GARRAIA_CONFIG_DIR` para um diretorio proprio.

use std::path::{Path, PathBuf};

const ESTE_ARQUIVO: &str = "testes_que_sobem_o_gateway_isolam_o_config_dir.rs";

fn fontes_rs(dir: &Path, saida: &mut Vec<PathBuf>) {
    let entradas = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("ler {}: {e}", dir.display()));
    for entrada in entradas {
        let caminho = entrada.expect("entrada de diretorio").path();
        if caminho.is_dir() {
            fontes_rs(&caminho, saida);
        } else if caminho.extension().is_some_and(|ext| ext == "rs") {
            saida.push(caminho);
        }
    }
}

fn isola_o_config_dir(fonte: &str) -> bool {
    fonte.contains("config_dir_descartavel::isolar()")
        || (fonte.contains("set_var") && fonte.contains("\"GARRAIA_CONFIG_DIR\""))
}

#[test]
fn quem_sobe_o_gateway_isola_o_config_dir() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("o gateway mora em crates/");
    let mut fontes = Vec::new();
    for crate_dir in std::fs::read_dir(crates).expect("listar crates/") {
        let tests = crate_dir.expect("entrada de crates/").path().join("tests");
        if tests.is_dir() {
            fontes_rs(&tests, &mut fontes);
        }
    }

    let mut sobem = 0;
    let mut sem_isolamento = Vec::new();
    for caminho in &fontes {
        if caminho.file_name().is_some_and(|n| n == ESTE_ARQUIVO) {
            continue;
        }
        let fonte = std::fs::read_to_string(caminho)
            .unwrap_or_else(|e| panic!("ler {}: {e}", caminho.display()));
        if !fonte.contains("GatewayServer::new(") {
            continue;
        }
        sobem += 1;
        if !isola_o_config_dir(&fonte) {
            let relativo = caminho.strip_prefix(crates).unwrap_or(caminho);
            sem_isolamento.push(relativo.display().to_string());
        }
    }

    // Se a varredura nao achar ninguem, o teste passaria sem provar nada
    // (um rename, um caminho errado).
    assert!(
        sobem >= 5,
        "a varredura achou so {sobem} arquivo(s) que sobem o gateway; o caminho mudou?"
    );
    assert!(
        sem_isolamento.is_empty(),
        "estes testes sobem o GatewayServer com o config dir de quem roda cargo test \
         (admin.db, master.key e sessions.db reais); chame \
         config_dir_descartavel::isolar() antes de subir o servidor: {sem_isolamento:#?}"
    );
}
