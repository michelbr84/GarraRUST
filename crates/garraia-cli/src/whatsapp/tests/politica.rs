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
