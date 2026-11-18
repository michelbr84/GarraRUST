//! #1915 (sugestao da revisao da #1948): o painel Sessions > Capabilities do
//! console admin traduz `denied_by` / `denied_because` em rotulos por dois
//! mapas em JS (`camada` e `motivo`, em `src/admin.html`). Os mapas duplicam
//! os codigos de `garraia_agents::modes::{CamadaDaRecusa, MotivoDaPolitica}`,
//! e sem este teste um codigo novo sairia cru no console sem quebrar nada.
//!
//! A paridade e cobrada nos dois sentidos: todo codigo do Rust tem rotulo, e
//! nenhum rotulo aponta para um codigo que o gateway nao emite mais.

use std::collections::BTreeSet;

use garraia_agents::modes::{CamadaDaRecusa, MotivoDaPolitica};

const ADMIN_HTML: &str = include_str!("../src/admin.html");

/// Chaves do objeto literal atribuido a `var <nome> = ...`, na mesma linha.
/// Os rotulos nao tem virgula nem dois-pontos, entao o corte e simples; se um
/// dia tiverem, o teste falha com a chave estranha na mensagem.
fn chaves_do_mapa(nome: &str) -> BTreeSet<String> {
    let marca = format!("var {nome} = ");
    let linha = ADMIN_HTML
        .lines()
        .find(|l| l.trim_start().starts_with(&marca))
        .unwrap_or_else(|| panic!("admin.html nao declara `{marca}`"));
    let inicio = linha
        .find('{')
        .unwrap_or_else(|| panic!("`{nome}` sem objeto literal: {linha}"));
    let fim = linha[inicio..]
        .find('}')
        .map(|i| inicio + i)
        .unwrap_or_else(|| panic!("`{nome}` sem fecha-chaves: {linha}"));
    linha[inicio + 1..fim]
        .split(',')
        .map(|par| {
            let (chave, rotulo) = par
                .split_once(':')
                .unwrap_or_else(|| panic!("entrada sem `:` em `{nome}`: {par}"));
            assert!(
                rotulo.trim().starts_with('\'') && rotulo.trim().ends_with('\''),
                "rotulo de `{nome}` fora de aspas simples: {par}"
            );
            chave.trim().to_string()
        })
        .collect()
}

#[test]
fn rotulos_da_camada_espelham_camada_da_recusa() {
    let rust: BTreeSet<String> = CamadaDaRecusa::TODOS
        .iter()
        .map(|c| c.as_str().to_string())
        .collect();
    assert_eq!(chaves_do_mapa("camada"), rust);
}

#[test]
fn rotulos_do_motivo_espelham_motivo_da_politica() {
    let rust: BTreeSet<String> = MotivoDaPolitica::TODOS
        .iter()
        .map(|m| m.as_str().to_string())
        .collect();
    assert_eq!(chaves_do_mapa("motivo"), rust);
}

/// O lookup nao pode herdar de `Object.prototype`: `camada['constructor']`
/// devolveria uma funcao e o console exibiria o fonte dela.
#[test]
fn mapas_nao_tem_prototipo() {
    for nome in ["camada", "motivo"] {
        let marca = format!("var {nome} = Object.assign(Object.create(null), {{");
        assert!(
            ADMIN_HTML.contains(&marca),
            "`{nome}` precisa ser mapa sem prototipo"
        );
    }
}
