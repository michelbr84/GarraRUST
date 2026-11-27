//! #2021 e #2027: todo teste que monta o gateway de verdade desvia o config dir.
//!
//! `GatewayServer::run()` abre o `admin.db`, resolve o `master.key` e grava o
//! `sessions.db` sob o `<config_dir>`: o de `GARRAIA_CONFIG_DIR` ou, sem ele,
//! o XDG de quem roda `cargo test` (`~/.config/garraia`). Um alvo de teste que
//! sobe o servidor sem desviar esse diretorio trabalha sobre os dados reais
//! do operador. Com um `master.key` de tamanho errado e o `admin.db` real
//! vazio, por exemplo, o boot troca a chave do operador por outra.
//!
//! O `AppState::new` (#2027) tambem resolve o config dir padrao: le o
//! `mcp.json` e o `allowlist.json` reais, provisiona um `mcp.json` e o
//! `data/workspace/` quando o `mcp.json` nao existe e liga o cofre real ao
//! registry do MCP.
//!
//! O teste varre os `tests/` de todas as crates.
//!
//! - Quem chama `GatewayServer::new` precisa chamar
//!   `config_dir_descartavel::isolar()` ou apontar `GARRAIA_CONFIG_DIR` para
//!   um diretorio proprio.
//! - Quem chama `AppState::new` cai na mesma regra. O caminho preferido e nao
//!   chamar: `config_dir_descartavel::app_state(...)` monta o estado com
//!   `AppState::with_config_dir` sobre um diretorio descartavel, sem mexer no
//!   ambiente do processo.
//!
//! A checagem e textual, por arquivo, e erra para o lado seguro: um teste que
//! isole por outro caminho (um helper em outro modulo) e reprovado ate
//! `isola_o_config_dir` aprender esse caminho. Ela nao confere a ordem (isolar
//! depois de subir passaria) nem pega quem abre o `admin.db` real sem subir o
//! gateway.

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

/// Os arquivos de `crates/*/tests/` que contem `chamada`: quantos sao e quais
/// deles nao isolam o config dir, com o caminho relativo a `crates/`.
fn sem_isolamento(chamada: &str) -> (usize, Vec<String>) {
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

    let mut chamam = 0;
    let mut sem_isolamento = Vec::new();
    for caminho in &fontes {
        if caminho.file_name().is_some_and(|n| n == ESTE_ARQUIVO) {
            continue;
        }
        let fonte = std::fs::read_to_string(caminho)
            .unwrap_or_else(|e| panic!("ler {}: {e}", caminho.display()));
        if !fonte.contains(chamada) {
            continue;
        }
        chamam += 1;
        if !isola_o_config_dir(&fonte) {
            let relativo = caminho.strip_prefix(crates).unwrap_or(caminho);
            sem_isolamento.push(relativo.display().to_string());
        }
    }
    sem_isolamento.sort();
    (chamam, sem_isolamento)
}

#[test]
fn quem_sobe_o_gateway_isola_o_config_dir() {
    let (sobem, sem_isolamento) = sem_isolamento("GatewayServer::new(");

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

#[test]
fn quem_monta_o_app_state_isola_o_config_dir() {
    let (montam, sem_isolamento) = sem_isolamento("AppState::new(");

    // Mesmo piso do teste acima: a varredura tem de achar alguem. Os arquivos
    // que ainda chamam `AppState::new` sao os que desviam o GARRAIA_CONFIG_DIR.
    assert!(
        montam >= 10,
        "a varredura achou so {montam} arquivo(s) que montam o AppState; o caminho mudou?"
    );
    assert!(
        sem_isolamento.is_empty(),
        "estes testes montam o AppState com o config dir de quem roda cargo test \
         (mcp.json, allowlist.json e cofre reais); troque o AppState::new por \
         config_dir_descartavel::app_state(...): {sem_isolamento:#?}"
    );
}
