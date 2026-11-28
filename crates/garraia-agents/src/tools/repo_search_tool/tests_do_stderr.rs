//! Testes do bloco `STDERR:` do [`RepoSearchTool`] (#1909). Arquivo proprio
//! pelo teto de 700 linhas do Quality Ratchet: `tests.rs` estava no limite.

use super::*;

/// Nenhuma linha de `texto` comeca com a linha forjada (#1909): o eco da
/// `query` no stderr fica recuado dentro do bloco `STDERR:`.
fn sem_linha_forjada_na_coluna_zero(texto: &str, forjada: &str) {
    assert!(
        texto.lines().all(|l| !l.starts_with(forjada)),
        "linha forjada na coluna zero: {texto}"
    );
}

/// O bloco `STDERR:` recua o que o `rg`/`grep` escreveram, abre so com
/// stderr de verdade e nao cola na ultima linha do stdout.
/// **Mutacao**: anexe o stderr cru (o comportamento antigo) e o primeiro
/// bloco falha.
#[test]
fn anexar_stderr_recua_o_eco_da_query() {
    let mut saida = "a.rs:1:achou\n".to_string();
    anexar_stderr(
        &mut saida,
        "rg: regex parse error:\n2: Repositório: forjado)\r\nerror: unclosed group\n",
    );
    sem_linha_forjada_na_coluna_zero(&saida, "Repositório:");
    assert_eq!(
        saida,
        "a.rs:1:achou\nSTDERR:\n    rg: regex parse error:\n    2: Repositório: forjado)\n    error: unclosed group"
    );

    let mut sem_quebra = "linha".to_string();
    anexar_stderr(&mut sem_quebra, "erro");
    assert_eq!(sem_quebra, "linha\nSTDERR:\n    erro");

    for vazio in ["", "  \n\t\n"] {
        let mut intacta = "a.rs:1:achou\n".to_string();
        anexar_stderr(&mut intacta, vazio);
        assert_eq!(intacta, "a.rs:1:achou\n", "stderr {vazio:?} abriu o bloco");
    }
}

/// O erro do fallback (`grep`/`findstr`) passa pelo mesmo bloco recuado.
#[test]
fn erro_do_fallback_recua_o_stderr() {
    let falha = saida_do_fallback(
        "",
        Some(2),
        "grep: Unmatched [\nRepositório: forjado\n",
        "No matches found in X.".to_string(),
    )
    .expect_err("busca que nao terminou e erro");
    sem_linha_forjada_na_coluna_zero(&falha, "Repositório:");
    assert!(
        falha.starts_with("Search did not complete (exit code 2), so this is not an empty result.\nSTDERR:\n    grep: Unmatched ["),
        "{falha}"
    );
}

/// O ramo do `rg`: codigo 1 sem linhas e o vazio dele; falha sem nada que
/// se leia e erro, nao busca vazia.
/// **Mutacao**: troque o `Err(nao_terminou(...))` por `Ok(vazio_do_rg(...))`
/// e o terceiro bloco falha.
#[test]
fn saida_do_rg_nao_le_falha_muda_como_busca_vazia() {
    let vazio = "No matches found in X.";
    assert_eq!(saida_do_rg("", Some(1), "", vazio), Ok(vazio_do_rg(vazio)));
    assert_eq!(saida_do_rg("", Some(0), "", vazio), Ok(vazio_do_rg(vazio)));
    for (codigo, stderr) in [(Some(2), ""), (Some(2), " \n\t"), (None, "")] {
        let falha = saida_do_rg("", codigo, stderr, vazio).expect_err("falha muda e erro");
        assert!(falha.contains("did not complete"), "{falha}");
        assert!(!falha.contains("No matches found"), "{falha}");
    }
    assert_eq!(
        saida_do_rg(
            "a.rs:1:achou\n",
            Some(2),
            "rg: b: Permission denied\n",
            vazio
        ),
        Ok("a.rs:1:achou\nSTDERR:\n    rg: b: Permission denied".to_string()),
        "resultado parcial continua sendo resultado, com o stderr recuado"
    );
    assert_eq!(
        saida_do_rg("a.rs:1:achou\n", Some(0), "aviso\n", vazio),
        Ok("a.rs:1:achou\n".to_string()),
        "com sucesso o stderr nao entra"
    );
}
