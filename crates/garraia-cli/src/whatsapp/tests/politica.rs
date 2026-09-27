//! `garraia whatsapp access ...` (#1396, #1397, #1398, #1399, #1400, #1401,
//! #1413, #1414): a CLI e uma casca fina sobre o motor do gateway
//! (`whatsapp_linked_politica::{mutacao, impacto, auditoria}`). O que se
//! prova aqui e o contrato de operador: exit codes, o que vai (e nao vai)
//! para o `config.yml`, o audit, o `--dry-run` e a saida sem identidade.

use super::*;
use crate::whatsapp::politica::{
    self as acesso_v2, Aplicacao, ComandoDeAcesso, json_da_politica, json_do_audit,
    linhas_da_politica, linhas_de_impacto,
};
use garraia_agents::modes::Nivel;
use garraia_config::ExecutionProfile;
use garraia_gateway::bootstrap::whatsapp_linked_politica::mutacao::Mutacao;
use garraia_gateway::bootstrap::whatsapp_linked_politica::{
    Admission, Alcance, Principal, auditoria, principal_do_turno,
};
use garraia_gateway::bootstrap::{WhatsAppLinkedSettings, whatsapp_linked_settings};

const DONO: &str = "5511977776666";
const ESTRANHO: &str = "5521955554444";
const GRUPO: &str = "120363000000000000@g.us";

fn preparar(dir: &tempfile::TempDir, settings: serde_json::Value) -> Context {
    let ctx = ctx_in(dir, false);
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");
    loader
        .save(&config_com_linked(None, settings))
        .expect("save");
    ctx
}

fn settings_de(ctx: &Context) -> WhatsAppLinkedSettings {
    let config = ctx
        .loader
        .as_ref()
        .expect("loader")
        .load_sem_env()
        .expect("load");
    whatsapp_linked_settings(&config)
}

fn principal(ctx: &Context, quem: &str) -> Principal {
    principal_do_turno(&settings_de(ctx), quem, "x", false, false)
}

fn audit(ctx: &Context) -> Vec<auditoria::Evento> {
    auditoria::ler(&ctx.data_dir, 50).expect("ler audit")
}

fn arquivo_de_audit(ctx: &Context) -> String {
    std::fs::read_to_string(ctx.data_dir.join(auditoria::ARQUIVO)).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// level / write
// ---------------------------------------------------------------------------

#[test]
fn level_e_write_gravam_no_config_e_auditam_sem_identidade() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(&dir, serde_json::json!({ "allow": [NUMERO] }));
    let p = ScriptedPrompter::default();

    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Nivel {
            numero: "+55 11 99999-8888".to_string(),
            nivel: Nivel::Read,
            dry_run: false,
        },
    );
    assert_eq!(code, 0);
    assert_eq!(
        principal(&ctx, NUMERO),
        Principal::Usuario(Alcance::LEITURA)
    );
    let eventos = audit(&ctx);
    assert_eq!(eventos.len(), 1);
    assert_eq!(eventos[0].acao, "level");
    assert_eq!(eventos[0].origem, "cli");
    assert_eq!(eventos[0].alvo.as_deref(), Some("…8888"));
    assert!(!eventos[0].ator.is_empty(), "quem rodou o comando");

    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Write {
            numero: format!("+{NUMERO}"),
            on: true,
            dry_run: false,
        },
    );
    assert_eq!(code, 0);
    assert_eq!(
        principal(&ctx, NUMERO),
        Principal::Usuario(Alcance {
            nivel: Nivel::Read,
            write: true
        })
    );
    let eventos = audit(&ctx);
    assert_eq!(eventos.len(), 2);
    assert_eq!(eventos[0].acao, "write", "mais recente primeiro");
    let bruto = arquivo_de_audit(&ctx);
    assert!(!bruto.contains(NUMERO), "audit com numero inteiro: {bruto}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let modo = std::fs::metadata(ctx.data_dir.join(auditoria::ARQUIVO))
            .expect("meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(modo, 0o600);
    }
}

#[test]
fn write_em_desconhecido_e_numero_invalido_dao_65_sem_gravar() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(&dir, serde_json::json!({ "allow": [NUMERO] }));
    let p = ScriptedPrompter::default();
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Write {
            numero: format!("+{ESTRANHO}"),
            on: true,
            dry_run: false,
        },
    );
    assert_eq!(code, acesso::EX_DATAERR);
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Nivel {
            numero: "abc".to_string(),
            nivel: Nivel::Read,
            dry_run: false,
        },
    );
    assert_eq!(code, acesso::EX_DATAERR);
    assert!(audit(&ctx).is_empty(), "recusa nao audita");
    assert_eq!(principal(&ctx, ESTRANHO), Principal::Estranho);
}

// ---------------------------------------------------------------------------
// --dry-run
// ---------------------------------------------------------------------------

#[test]
fn dry_run_mostra_o_impacto_pelo_motor_real_sem_gravar_nem_auditar() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(
        &dir,
        serde_json::json!({ "default_mode": "code", "allow": [NUMERO] }),
    );
    let mutacao = Mutacao::Write {
        identidade: NUMERO.to_string(),
        on: false,
    };
    let ap: Aplicacao = acesso_v2::aplicar(&ctx, &mutacao, true).expect("aplica em seco");
    assert!(!ap.gravou);
    assert!(ap.aplicada.mudou);
    let perde: Vec<&str> = ap
        .diferencas
        .iter()
        .flat_map(|d| d.perde.iter().copied())
        .collect();
    assert!(perde.contains(&"escrita de arquivo"), "{perde:?}");
    assert!(perde.contains(&"MCP escrita"), "{perde:?}");
    let texto = linhas_de_impacto(Lang::Pt, &ap).join("\n");
    assert!(texto.contains("perde"), "{texto}");
    assert!(texto.contains("escrita de arquivo"), "{texto}");
    assert!(!texto.contains(NUMERO), "impacto com numero: {texto}");
    // Nada mudou no disco, nada foi auditado.
    assert_eq!(
        principal(&ctx, NUMERO),
        Principal::Usuario(Alcance::COMPLETO)
    );
    assert!(audit(&ctx).is_empty());
    assert!(!ctx.data_dir.join(auditoria::ARQUIVO).exists());
    // E pelo comando, com exit 0.
    let p = ScriptedPrompter::default();
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Write {
            numero: format!("+{NUMERO}"),
            on: false,
            dry_run: true,
        },
    );
    assert_eq!(code, 0);
    assert_eq!(
        principal(&ctx, NUMERO),
        Principal::Usuario(Alcance::COMPLETO)
    );
}

// ---------------------------------------------------------------------------
// open / restricted / default
// ---------------------------------------------------------------------------

#[test]
fn open_exige_yes_sem_terminal_confirmacao_no_terminal_e_avisa() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut ctx = preparar(&dir, serde_json::json!({ "allow": [NUMERO] }));
    let p = ScriptedPrompter::default();
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Abrir {
            yes: false,
            dry_run: false,
        },
    );
    assert_eq!(code, acesso::EX_USAGE, "sem terminal e sem --yes");
    assert_eq!(settings_de(&ctx).access.admission, Admission::Restricted);
    assert!(audit(&ctx).is_empty());

    ctx.interactive = true;
    let nao = ScriptedPrompter::with_confirms(&[false]);
    let code = acesso_v2::access(
        &ctx,
        &nao,
        &ComandoDeAcesso::Abrir {
            yes: false,
            dry_run: false,
        },
    );
    assert_eq!(code, EX_CANCELLED);
    let pergunta = nao.seen_prompts.borrow().join(" ");
    assert!(
        pergunta.to_lowercase().contains("qualquer"),
        "a pergunta avisa que qualquer pessoa passa a entrar: {pergunta}"
    );
    assert_eq!(settings_de(&ctx).access.admission, Admission::Restricted);

    let sim = ScriptedPrompter::with_confirms(&[true]);
    let code = acesso_v2::access(
        &ctx,
        &sim,
        &ComandoDeAcesso::Abrir {
            yes: false,
            dry_run: false,
        },
    );
    assert_eq!(code, 0);
    assert_eq!(settings_de(&ctx).access.admission, Admission::Open);
    assert_eq!(
        principal(&ctx, ESTRANHO),
        Principal::Desconhecido(Alcance::CHAT)
    );
    assert_eq!(
        principal(&ctx, NUMERO),
        Principal::Usuario(Alcance::COMPLETO),
        "explicitos preservados"
    );
    assert_eq!(audit(&ctx)[0].acao, "open");

    let code = acesso_v2::access(&ctx, &p, &ComandoDeAcesso::Restringir { dry_run: false });
    assert_eq!(code, 0);
    assert_eq!(settings_de(&ctx).access.admission, Admission::Restricted);
    assert_eq!(audit(&ctx)[0].acao, "restricted");
}

#[test]
fn default_full_e_recusado_e_o_default_read_so_vale_em_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(&dir, serde_json::json!({ "allow": [NUMERO] }));
    let p = ScriptedPrompter::default();
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Default {
            nivel: Nivel::Full,
            write: false,
            dry_run: false,
        },
    );
    assert_eq!(code, acesso::EX_DATAERR);
    assert!(audit(&ctx).is_empty());
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Default {
            nivel: Nivel::Read,
            write: false,
            dry_run: false,
        },
    );
    assert_eq!(code, 0);
    assert_eq!(
        principal(&ctx, ESTRANHO),
        Principal::Estranho,
        "restricted guarda sem aplicar"
    );
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Abrir {
            yes: true,
            dry_run: false,
        },
    );
    assert_eq!(code, 0);
    assert_eq!(
        principal(&ctx, ESTRANHO),
        Principal::Desconhecido(Alcance::LEITURA)
    );
}

// ---------------------------------------------------------------------------
// block / unblock / groups / reset
// ---------------------------------------------------------------------------

#[test]
fn block_e_unblock_valem_e_auditam() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(&dir, serde_json::json!({ "allow": [NUMERO] }));
    let p = ScriptedPrompter::default();
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Bloquear {
            numero: format!("+{NUMERO}"),
            dry_run: false,
        },
    );
    assert_eq!(code, 0);
    assert_eq!(principal(&ctx, NUMERO), Principal::Bloqueado);
    assert_eq!(audit(&ctx)[0].acao, "block");
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Desbloquear {
            numero: format!("+{NUMERO}"),
            dry_run: false,
        },
    );
    assert_eq!(code, 0);
    assert_eq!(
        principal(&ctx, NUMERO),
        Principal::Usuario(Alcance::COMPLETO)
    );
    assert_eq!(audit(&ctx)[0].acao, "unblock");
}

#[test]
fn grupos_ligam_desligam_e_recebem_politica_por_jid() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(&dir, serde_json::json!({ "owners": [DONO] }));
    let p = ScriptedPrompter::default();
    assert_eq!(
        acesso_v2::access(
            &ctx,
            &p,
            &ComandoDeAcesso::Grupos {
                ligados: true,
                dry_run: false
            }
        ),
        0
    );
    assert!(settings_de(&ctx).responde_em_grupo());
    assert_eq!(
        acesso_v2::access(
            &ctx,
            &p,
            &ComandoDeAcesso::GrupoDefault {
                nivel: Nivel::Chat,
                write: false,
                dry_run: false
            }
        ),
        0
    );
    assert_eq!(
        acesso_v2::access(
            &ctx,
            &p,
            &ComandoDeAcesso::Grupo {
                jid: GRUPO.to_string(),
                nivel: Nivel::Read,
                write: false,
                dry_run: false
            }
        ),
        0
    );
    let st = settings_de(&ctx);
    assert_eq!(
        principal_do_turno(&st, DONO, GRUPO, true, false),
        Principal::Grupo(Alcance::LEITURA)
    );
    assert_eq!(
        principal_do_turno(&st, DONO, "outro@g.us", true, false),
        Principal::Grupo(Alcance::CHAT)
    );
    // JID que nao e de grupo: recusado, sem gravar.
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Grupo {
            jid: "abc".to_string(),
            nivel: Nivel::Read,
            write: false,
            dry_run: false,
        },
    );
    assert_eq!(code, acesso::EX_DATAERR);
    assert_eq!(
        acesso_v2::access(
            &ctx,
            &p,
            &ComandoDeAcesso::Grupos {
                ligados: false,
                dry_run: false
            }
        ),
        0
    );
    assert!(!settings_de(&ctx).responde_em_grupo());
    assert_eq!(audit(&ctx)[0].acao, "groups");
}

#[test]
fn reset_exige_confirmacao_volta_ao_seguro_e_e_idempotente() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(
        &dir,
        serde_json::json!({
            "allow": [NUMERO], "owners": [DONO], "reply_in_groups": true,
            "access": { "admission": "open", "default": { "level": "read" },
                        "users": { NUMERO: { "level": "full", "write": true }, ESTRANHO: { "blocked": true } } }
        }),
    );
    let p = ScriptedPrompter::default();
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Reset {
            yes: false,
            dry_run: false,
        },
    );
    assert_eq!(code, acesso::EX_USAGE, "sem terminal e sem --yes");
    assert_eq!(
        settings_de(&ctx).access.admission,
        Admission::Open,
        "nada mudou"
    );

    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Reset {
            yes: true,
            dry_run: false,
        },
    );
    assert_eq!(code, 0);
    let st = settings_de(&ctx);
    assert_eq!(st.access.admission, Admission::Restricted);
    assert_eq!(st.access.default, Alcance::CHAT);
    assert!(!st.responde_em_grupo());
    assert_eq!(
        principal(&ctx, NUMERO),
        Principal::Usuario(Alcance::COMPLETO),
        "override saiu, allow fica"
    );
    assert_eq!(principal(&ctx, DONO), Principal::Dono, "dono fica");
    assert_eq!(
        principal(&ctx, ESTRANHO),
        Principal::Bloqueado,
        "bloqueio fica"
    );
    let eventos = audit(&ctx);
    assert_eq!(eventos.len(), 1);
    assert_eq!(eventos[0].acao, "reset");

    // Idempotente: nada muda, nada e auditado, exit 0.
    let code = acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Reset {
            yes: true,
            dry_run: false,
        },
    );
    assert_eq!(code, 0);
    assert_eq!(audit(&ctx).len(), 1);
}

// ---------------------------------------------------------------------------
// show / audit
// ---------------------------------------------------------------------------

#[test]
fn mostrar_da_a_politica_efetiva_sem_identidade_salvo_com_reveal() {
    let config = config_com_linked(
        Some(ExecutionProfile::IsolatedPod),
        serde_json::json!({
            "allow": [NUMERO], "owners": [DONO],
            "access": { "admission": "open", "users": { ESTRANHO: { "blocked": true } },
                        "groups": { "enabled": true, GRUPO: { "level": "chat" } } }
        }),
    );
    let doc = json_da_politica(&config, ExecutionProfile::IsolatedPod, false);
    assert_eq!(doc["admission"], serde_json::json!("open"));
    assert_eq!(doc["execution_profile"], serde_json::json!("isolated-pod"));
    let texto = doc.to_string();
    for numero in [NUMERO, DONO, ESTRANHO, "120363000000000000"] {
        assert!(!texto.contains(numero), "json com identidade: {texto}");
    }
    let principais = doc["principals"].as_array().expect("lista");
    let etiquetas: Vec<&str> = principais
        .iter()
        .filter_map(|p| p["principal"].as_str())
        .collect();
    for esperado in [
        "dono",
        "usuario",
        "bloqueado",
        "pareado",
        "desconhecido",
        "grupo",
    ] {
        assert!(etiquetas.contains(&esperado), "{etiquetas:?}");
    }
    let dono = principais
        .iter()
        .find(|p| p["principal"] == "dono")
        .expect("dono");
    assert_eq!(dono["mode"], serde_json::json!("code"), "dono em pod");
    assert_eq!(dono["last4"], serde_json::json!("…6666"));
    assert!(
        dono.get("identity").is_none(),
        "sem --reveal nao ha identidade"
    );

    let revelado = json_da_politica(&config, ExecutionProfile::IsolatedPod, true);
    assert!(
        revelado.to_string().contains(DONO),
        "--reveal mostra o valor configurado"
    );

    let linhas =
        linhas_da_politica(Lang::Pt, &config, ExecutionProfile::IsolatedPod, false).join("\n");
    for palavra in [
        "open",
        "dono",
        "usuario",
        "desconhecido",
        "grupo",
        "isolated-pod",
    ] {
        assert!(linhas.contains(palavra), "faltou `{palavra}` em:\n{linhas}");
    }
    assert!(!linhas.contains(NUMERO));

    // Pelo comando.
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(&dir, serde_json::json!({ "allow": [NUMERO] }));
    let p = ScriptedPrompter::default();
    assert_eq!(
        acesso_v2::access(
            &ctx,
            &p,
            &ComandoDeAcesso::Mostrar {
                json: true,
                revelar: false
            }
        ),
        0
    );
    assert_eq!(
        acesso_v2::access(
            &ctx,
            &p,
            &ComandoDeAcesso::Mostrar {
                json: false,
                revelar: true
            }
        ),
        0
    );
}

#[test]
fn audit_lista_mais_recente_primeiro_e_e_vazio_sem_arquivo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(&dir, serde_json::json!({ "allow": [NUMERO] }));
    assert_eq!(
        json_do_audit(&ctx, 10).expect("le")["events"],
        serde_json::json!([]),
        "sem arquivo, lista vazia"
    );
    let p = ScriptedPrompter::default();
    acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Nivel {
            numero: format!("+{NUMERO}"),
            nivel: Nivel::Read,
            dry_run: false,
        },
    );
    acesso_v2::access(
        &ctx,
        &p,
        &ComandoDeAcesso::Write {
            numero: format!("+{NUMERO}"),
            on: true,
            dry_run: false,
        },
    );
    let doc = json_do_audit(&ctx, 10).expect("le");
    let eventos = doc["events"].as_array().expect("lista");
    assert_eq!(eventos.len(), 2);
    assert_eq!(eventos[0]["action"], serde_json::json!("write"));
    assert_eq!(eventos[1]["action"], serde_json::json!("level"));
    assert!(!doc.to_string().contains(NUMERO));
    assert_eq!(
        json_do_audit(&ctx, 1).expect("le")["events"]
            .as_array()
            .expect("lista")
            .len(),
        1
    );
    assert_eq!(
        acesso_v2::access(
            &ctx,
            &p,
            &ComandoDeAcesso::Audit {
                json: true,
                limit: 5
            }
        ),
        0
    );
    assert_eq!(
        acesso_v2::access(
            &ctx,
            &p,
            &ComandoDeAcesso::Audit {
                json: false,
                limit: 5
            }
        ),
        0
    );
}

// ---------------------------------------------------------------------------
// #1414: os comandos legados tambem auditam
// ---------------------------------------------------------------------------

/// Um evento por escrita que de fato mudou algo, com o nome do subcomando
/// como acao, `cli` como origem e o alvo mascarado — nunca a identidade.
fn ultimo_evento(ctx: &Context, acao: &str) -> auditoria::Evento {
    let eventos = audit(ctx);
    let e = eventos.first().cloned().expect("ha evento");
    assert_eq!(e.acao, acao);
    assert_eq!(e.origem, "cli");
    assert_eq!(e.alvo.as_deref(), Some("…8888"));
    assert!(!e.ator.is_empty(), "quem rodou o comando");
    e
}

/// A entrada de `…8888` no resumo `depois` de um evento.
fn entrada_no_depois(e: &auditoria::Evento) -> Option<serde_json::Value> {
    e.depois["users"]
        .as_array()
        .and_then(|us| us.iter().find(|u| u["alvo"] == "…8888").cloned())
}

/// `allow` e `remove` gravam `allow` por fora do motor de mutacao — e ate a
/// #1414 gravavam SEM audit. Agora cada escrita que muda algo deixa um
/// evento; a repeticao idempotente (nada mudou) nao deixa nada.
#[test]
fn allow_e_remove_legados_auditam_com_o_nome_do_subcomando() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(&dir, serde_json::json!({}));
    let p = ScriptedPrompter::default();

    assert_eq!(
        run(Action::Allow(pedido(ENTRADA, false, false)), &ctx, &p),
        0
    );
    assert_eq!(lista(&ctx, "allow"), vec![NUMERO.to_string()]);
    let e = ultimo_evento(&ctx, "allow");
    assert_eq!(audit(&ctx).len(), 1);
    let entrada = entrada_no_depois(&e).expect("…8888 no depois");
    assert_eq!(entrada["owner"], serde_json::json!(false));
    assert!(
        entrada_no_depois(&auditoria::Evento {
            depois: e.antes.clone(),
            ..e.clone()
        })
        .is_none(),
        "no antes …8888 ainda nao existia"
    );

    // De novo: ja estava, nada mudou, nada e auditado.
    assert_eq!(
        run(Action::Allow(pedido(ENTRADA, false, false)), &ctx, &p),
        0
    );
    assert_eq!(audit(&ctx).len(), 1, "o que nao mudou nao e auditado");

    assert_eq!(run(Action::Remove(remocao(ENTRADA, false)), &ctx, &p), 0);
    let e = ultimo_evento(&ctx, "remove");
    assert_eq!(audit(&ctx).len(), 2);
    assert!(entrada_no_depois(&e).is_none(), "saiu: {}", e.depois);

    // Remover quem nao esta: sai 0 e nao audita.
    assert_eq!(run(Action::Remove(remocao(ENTRADA, false)), &ctx, &p), 0);
    assert_eq!(audit(&ctx).len(), 2);

    let bruto = arquivo_de_audit(&ctx);
    assert!(!bruto.contains(NUMERO), "audit com numero inteiro: {bruto}");
}

/// `owner` e `unowner` idem: `owner` (so no pod) e `unowner` deixam o nome do
/// subcomando, e o `depois` diz o papel que ficou.
#[test]
fn owner_e_unowner_legados_auditam_com_o_nome_do_subcomando() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    grava_config(
        &ctx,
        pod(),
        Some(serde_json::json!({ "allow": [NUMERO] })),
        Some(true),
    );
    let p = ScriptedPrompter::default();

    assert_eq!(run(Action::Owner(papel(ENTRADA, true)), &ctx, &p), 0);
    let e = ultimo_evento(&ctx, "owner");
    assert_eq!(audit(&ctx).len(), 1);
    assert_eq!(
        entrada_no_depois(&e).expect("…8888")["owner"],
        serde_json::json!(true)
    );
    // Ja era dono: nada muda, nada e auditado.
    assert_eq!(run(Action::Owner(papel(ENTRADA, true)), &ctx, &p), 0);
    assert_eq!(audit(&ctx).len(), 1);

    assert_eq!(run(Action::Unowner(papel(ENTRADA, true)), &ctx, &p), 0);
    let e = ultimo_evento(&ctx, "unowner");
    assert_eq!(audit(&ctx).len(), 2);
    let entrada = entrada_no_depois(&e).expect("o acesso sobrevive ao rebaixamento");
    assert_eq!(entrada["owner"], serde_json::json!(false));
    // Nao era mais dono: nada muda, nada e auditado.
    assert_eq!(run(Action::Unowner(papel(ENTRADA, true)), &ctx, &p), 0);
    assert_eq!(audit(&ctx).len(), 2);

    let bruto = arquivo_de_audit(&ctx);
    assert!(!bruto.contains(NUMERO), "audit com numero inteiro: {bruto}");
}

/// `allow --owner` e o subcomando `allow`: a acao e `allow`, e e o resumo
/// `depois` que diz que o alvo entrou como dono.
#[test]
fn allow_owner_audita_como_allow_e_o_depois_diz_dono() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    grava_config(&ctx, pod(), Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default();
    assert_eq!(run(Action::Allow(pedido(ENTRADA, true, true)), &ctx, &p), 0);
    let e = ultimo_evento(&ctx, "allow");
    assert_eq!(
        entrada_no_depois(&e).expect("…8888")["owner"],
        serde_json::json!(true)
    );
    assert!(!arquivo_de_audit(&ctx).contains(NUMERO));
}

/// A recusa (65) e o cancelamento nao auditam — nada foi gravado.
#[test]
fn comandos_legados_recusados_nao_auditam() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(&dir, serde_json::json!({ "owners": [NUMERO] }));
    let p = ScriptedPrompter::default();
    assert_eq!(
        run(Action::Allow(pedido("abc", false, false)), &ctx, &p),
        acesso::EX_DATAERR
    );
    assert_eq!(
        run(Action::Remove(remocao(ENTRADA, false)), &ctx, &p),
        acesso::EX_USAGE,
        "dono sem terminal e sem --yes"
    );
    assert!(audit(&ctx).is_empty());
    assert!(!ctx.data_dir.join(auditoria::ARQUIVO).exists());
}

/// O mesmo contrato dos comandos novos: a mudanca gravada com o audit
/// indisponivel sai 73 — a config ficou, e o operador fica sabendo.
#[test]
fn comando_legado_com_audit_indisponivel_grava_e_sai_73() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = preparar(&dir, serde_json::json!({}));
    // Um ARQUIVO onde o audit quer um diretorio: `registrar` nao consegue
    // criar `audit/`.
    std::fs::create_dir_all(&ctx.data_dir).expect("data dir");
    std::fs::write(ctx.data_dir.join("audit"), b"no lugar do diretorio").expect("arquivo");
    let p = ScriptedPrompter::default();
    assert_eq!(
        run(Action::Allow(pedido(ENTRADA, false, false)), &ctx, &p),
        73
    );
    assert_eq!(
        lista(&ctx, "allow"),
        vec![NUMERO.to_string()],
        "a mudanca foi gravada mesmo assim"
    );
}

// ---------------------------------------------------------------------------
// #1429: o wizard `link` oferece a politica depois do QR
// ---------------------------------------------------------------------------

/// As acoes do audit, mais recente primeiro.
fn acoes(ctx: &Context) -> Vec<String> {
    audit(ctx).into_iter().map(|e| e.acao).collect()
}

/// Enter em tudo: o numero entra com `read` sem escrita e a admissao fica
/// `restricted` — pelo motor e pelo audit, como o `level` faria.
#[test]
fn wizard_com_defaults_grava_read_sem_escrita_e_restricted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default().with_inputs(&[ENTRADA]);

    let pos = pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert_eq!(pos.autorizados, 1);
    assert!(p.asked("Nível de acesso"), "{:?}", p.seen_prompts.borrow());
    assert!(
        p.asked("escrita de arquivo"),
        "write e perguntado fora de chat"
    );
    assert!(p.asked("Admissão"), "{:?}", p.seen_prompts.borrow());
    assert_eq!(
        principal(&ctx, NUMERO),
        Principal::Usuario(Alcance::LEITURA),
        "default seguro: read, sem escrita"
    );
    assert_eq!(settings_de(&ctx).access.admission, Admission::Restricted);
    // `write off` e `restricted` nao mudam nada: nao viram evento.
    assert_eq!(acoes(&ctx), vec!["level", "allow"]);
    assert!(!arquivo_de_audit(&ctx).contains(NUMERO));
    let texto = pos.resumo.join("\n");
    assert!(texto.contains("restricted"), "{texto}");
    assert!(texto.contains("…8888"), "{texto}");
    assert!(!texto.contains(NUMERO), "{texto}");
}

/// `full` + escrita + `open` confirmado: os tres gravados, na ordem, e a
/// abertura passou pelo MESMO aviso do `access open`.
#[test]
fn wizard_com_full_write_e_open_grava_os_tres_e_avisa() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default()
        .with_inputs(&[ENTRADA])
        .with_selects(&[2, 1])
        .and_confirms(&[true, true]);

    let pos = pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert_eq!(
        principal(&ctx, NUMERO),
        Principal::Usuario(Alcance::COMPLETO)
    );
    assert!(
        p.asked("QUALQUER"),
        "o aviso de `open`: {:?}",
        p.seen_prompts.borrow()
    );
    assert_eq!(settings_de(&ctx).access.admission, Admission::Open);
    assert_eq!(
        principal(&ctx, ESTRANHO),
        Principal::Desconhecido(Alcance::CHAT),
        "o desconhecido entra com o default"
    );
    assert_eq!(acoes(&ctx), vec!["open", "write", "level", "allow"]);
    let texto = pos.resumo.join("\n");
    assert!(texto.contains("open"), "{texto}");
    assert!(!texto.contains(NUMERO), "{texto}");
}

/// `open` escolhido na lista, mas nao confirmado no aviso: fica `restricted`.
#[test]
fn wizard_open_escolhido_mas_nao_confirmado_fica_restricted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default()
        .with_inputs(&[ENTRADA])
        .with_selects(&[1, 1]);

    pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert!(p.asked("QUALQUER"));
    assert_eq!(settings_de(&ctx).access.admission, Admission::Restricted);
    assert!(!acoes(&ctx).iter().any(|a| a == "open"));
}

/// `chat` nao tem onde escrever: a pergunta de escrita nem aparece.
#[test]
fn wizard_em_chat_nao_pergunta_escrita() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default()
        .with_inputs(&[ENTRADA])
        .with_selects(&[0]);

    pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert!(
        !p.asked("escrita de arquivo"),
        "{:?}",
        p.seen_prompts.borrow()
    );
    assert_eq!(principal(&ctx, NUMERO), Principal::Usuario(Alcance::CHAT));
    assert_eq!(acoes(&ctx), vec!["level", "allow"]);
}

/// `link --allow <numero>` continua scriptavel: nenhuma pergunta, o numero
/// entra como sempre entrou (sem teto), e so o `allow` e auditado.
#[test]
fn wizard_com_allow_pre_respondido_nao_pergunta_politica() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default();

    let pos = pos_link(&ctx, &p, None, &pedido(ENTRADA, false, false)).expect("ok");
    assert_eq!(pos.autorizados, 1);
    assert!(
        p.seen_prompts.borrow().is_empty(),
        "nada e perguntado: {:?}",
        p.seen_prompts.borrow()
    );
    assert_eq!(
        principal(&ctx, NUMERO),
        Principal::Usuario(Alcance::COMPLETO),
        "sem pergunta, sem teto — como o `allow`"
    );
    assert_eq!(acoes(&ctx), vec!["allow"]);
}

/// No pod, quem vira dono nao recebe pergunta de nivel (dono nao tem teto;
/// o motor recusaria com `EDono`), mas a admissao e oferecida do mesmo jeito.
#[test]
fn wizard_dono_no_pod_nao_pergunta_nivel_mas_pergunta_admissao() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, pod(), Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default()
        .with_inputs(&[ENTRADA])
        .and_confirms(&[true]);

    pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert!(p.asked("DONO"));
    assert!(!p.asked("Nível de acesso"), "{:?}", p.seen_prompts.borrow());
    assert!(!p.asked("escrita de arquivo"));
    assert!(p.asked("Admissão"));
    assert_eq!(principal(&ctx, NUMERO), Principal::Dono);
    assert_eq!(acoes(&ctx), vec!["allow"]);
}

/// Resposta vazia no numero: nao ha para quem perguntar nivel, mas a
/// admissao continua sendo decisao do wizard.
#[test]
fn wizard_sem_numero_ainda_pergunta_admissao() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default().with_inputs(&[""]);

    let pos = pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert_eq!(pos.autorizados, 0);
    assert!(!p.asked("Nível de acesso"));
    assert!(p.asked("Admissão"));
    assert!(audit(&ctx).is_empty(), "nada mudou, nada auditado");
}

/// A funcao pura por tras do wizard: devolve as mutacoes na ordem em que
/// foram respondidas e nao toca no disco — quem grava e o `aplicar`.
#[test]
fn perguntar_politica_devolve_as_mutacoes_na_ordem_sem_tocar_no_disco() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let antes = std::fs::read_dir(dir.path())
        .map(|d| d.count())
        .unwrap_or(0);

    // Tudo no default: read (sem write, que nem e mutacao) e restricted.
    let p = ScriptedPrompter::default();
    let mutacoes = acesso_v2::perguntar_politica(&ctx, &p, Some(NUMERO), Alcance::CHAT);
    assert_eq!(
        mutacoes,
        vec![
            Mutacao::Nivel {
                identidade: NUMERO.to_string(),
                nivel: Nivel::Read
            },
            Mutacao::Admissao(Admission::Restricted),
        ]
    );
    let prompts = p.seen_prompts.borrow().join("\n");
    assert!(prompts.contains("…8888"), "{prompts}");
    assert!(
        !prompts.contains(NUMERO),
        "nunca a identidade inteira: {prompts}"
    );

    // full + write + open confirmado.
    let p = ScriptedPrompter::default()
        .with_selects(&[2, 1])
        .and_confirms(&[true, true]);
    let mutacoes = acesso_v2::perguntar_politica(&ctx, &p, Some(NUMERO), Alcance::LEITURA);
    assert_eq!(
        mutacoes,
        vec![
            Mutacao::Nivel {
                identidade: NUMERO.to_string(),
                nivel: Nivel::Full
            },
            Mutacao::Write {
                identidade: NUMERO.to_string(),
                on: true
            },
            Mutacao::Admissao(Admission::Open),
        ]
    );

    // Sem recem-autorizado: so a admissao. Em ingles tambem.
    let mut en = ctx_in(&dir, true);
    en.lang = Lang::En;
    let p = ScriptedPrompter::default();
    assert_eq!(
        acesso_v2::perguntar_politica(&en, &p, None, Alcance::CHAT),
        vec![Mutacao::Admissao(Admission::Restricted)]
    );
    assert!(p.asked("Admission"), "{:?}", p.seen_prompts.borrow());

    assert_eq!(
        std::fs::read_dir(dir.path())
            .map(|d| d.count())
            .unwrap_or(0),
        antes,
        "perguntar nao grava nada"
    );
    assert!(!ctx.data_dir.join(auditoria::ARQUIVO).exists());
}

/// A linha do default do desconhecido diz o valor REAL da config, nas duas
/// linguas, e aponta o comando que o muda.
#[test]
fn a_linha_do_default_do_desconhecido_diz_o_valor_real() {
    let pt = acesso_v2::linha_do_default_do_desconhecido(Lang::Pt, Alcance::LEITURA);
    assert!(pt.contains("read, write off"), "{pt}");
    assert!(pt.contains("whatsapp access default"), "{pt}");
    let en = acesso_v2::linha_do_default_do_desconhecido(Lang::En, Alcance::CHAT);
    assert!(en.contains("chat, write off"), "{en}");
    assert_ne!(pt, en);
}
