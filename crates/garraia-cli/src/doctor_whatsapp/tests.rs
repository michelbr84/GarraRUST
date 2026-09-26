//! O que a CLI ainda prova sozinha depois de o motor ir para o gateway
//! (#1420): a saida `--json` nao mudou de shape, o idioma detectado aqui vira
//! o idioma da tabela, e a tabela linha a linha continua testada — la, em
//! `garraia_gateway::bootstrap::whatsapp_linked_doctor`.

use super::*;

/// O `report` do `--json` e o contrato de script (#1419): `ok`, `exit_code`
/// e `report.{status,version,checks}` — e cada `check` com `id`, `status`,
/// `detail` e `next_step` so quando ha. E o MESMO `checks` que o console
/// recebe de `GET /admin/api/whatsapp/doctor`.
#[test]
fn o_payload_json_mantem_o_shape_do_report() {
    let linhas = vec![
        Linha {
            id: "whatsapp.linked",
            status: Semaforo::Error,
            detail: "nenhum WhatsApp pessoal vinculado".into(),
            next_step: Some("garraia whatsapp link".into()),
        },
        Linha {
            id: "mcp.visibility",
            status: Semaforo::NotConfigured,
            detail: "nenhum servidor MCP declarado".into(),
            next_step: None,
        },
    ];
    let code = exit_code(&linhas, false);
    assert_eq!(code, 69);
    let v = payload_json(&linhas, code);

    let mut topo: Vec<&str> = v
        .as_object()
        .expect("objeto")
        .keys()
        .map(String::as_str)
        .collect();
    topo.sort_unstable();
    assert_eq!(topo, ["exit_code", "ok", "report"]);
    assert_eq!(v["ok"], serde_json::json!(false));
    assert_eq!(v["exit_code"], serde_json::json!(69));

    let report = v["report"].as_object().expect("report");
    let mut chaves: Vec<&str> = report.keys().map(String::as_str).collect();
    chaves.sort_unstable();
    assert_eq!(chaves, ["checks", "status", "version"]);
    assert_eq!(v["report"]["status"], serde_json::json!("error"));
    assert_eq!(
        v["report"]["version"],
        serde_json::json!(env!("CARGO_PKG_VERSION"))
    );

    let checks = v["report"]["checks"].as_array().expect("checks");
    assert_eq!(checks.len(), 2);
    let mut c0: Vec<&str> = checks[0]
        .as_object()
        .expect("check")
        .keys()
        .map(String::as_str)
        .collect();
    c0.sort_unstable();
    assert_eq!(c0, ["detail", "id", "next_step", "status"]);
    let mut c1: Vec<&str> = checks[1]
        .as_object()
        .expect("check")
        .keys()
        .map(String::as_str)
        .collect();
    c1.sort_unstable();
    assert_eq!(c1, ["detail", "id", "status"], "next_step ausente some");
    assert_eq!(checks[1]["status"], serde_json::json!("not_configured"));

    // Tudo verde: `ok` e verdadeiro e o agregado e `ok`.
    let verde = vec![Linha {
        id: "whatsapp.access",
        status: Semaforo::Ok,
        detail: "autorizados: 2 · donos: 1".into(),
        next_step: None,
    }];
    let v = payload_json(&verde, exit_code(&verde, true));
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["exit_code"], serde_json::json!(0));
    assert_eq!(v["report"]["status"], serde_json::json!("ok"));
}

/// O idioma que a CLI detecta e o idioma em que a tabela fala.
#[test]
fn o_lang_da_cli_vira_o_lang_da_tabela() {
    assert_eq!(doctor::Lang::from(Lang::Pt), doctor::Lang::Pt);
    assert_eq!(doctor::Lang::from(Lang::En), doctor::Lang::En);
    // E a tabela, chamada com ele, fala a lingua certa.
    let fatos = doctor::Fatos::default();
    let pt = classificar(&fatos, Lang::Pt.into(), "garraia");
    let en = classificar(&fatos, Lang::En.into(), "garraia");
    let linha = |l: &[Linha]| {
        l.iter()
            .find(|l| l.id == "whatsapp.linked")
            .map(|l| l.detail.clone())
            .expect("whatsapp.linked")
    };
    assert_eq!(linha(&pt), "nenhum WhatsApp pessoal vinculado");
    assert_eq!(linha(&en), "no personal WhatsApp linked");
}

/// Os simbolos da tabela humana: unicode quando o locale afirma UTF-8, ASCII
/// de tres colunas quando nao — e nunca vazio.
#[test]
fn simbolo_tem_fallback_ascii() {
    for s in [
        Semaforo::Ok,
        Semaforo::Warning,
        Semaforo::Error,
        Semaforo::NotConfigured,
    ] {
        assert!(!simbolo(s, true).is_empty());
        assert!(simbolo(s, false).is_ascii(), "{:?}", simbolo(s, false));
        assert_eq!(simbolo(s, false).len(), 3);
    }
}
