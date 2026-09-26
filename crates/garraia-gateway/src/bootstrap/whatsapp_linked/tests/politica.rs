//! Access Policy v2 do WhatsApp pessoal (ADR 0025; #1388, #1390, #1391,
//! #1392, #1399, #1412, #1421, #1423).
//!
//! Tudo aqui e puro: secao de config → politica → principal → teto. O
//! `ToolGate` real fecha o circulo — o teto de um principal e testado contra
//! o inventario nativo e contra o piso `search` de hoje.

use std::collections::HashMap;

use garraia_agents::capacidades::{NATIVAS, capacidades_nativas};
use garraia_agents::modes::{Nivel, ToolGate};
use garraia_config::ChannelConfig;
use serde_json::json;

use crate::bootstrap::whatsapp_linked::politica::{
    Admission, Alcance, PoliticaDeAcesso, Principal, principal_do_turno, teto_do_principal,
};
use crate::bootstrap::whatsapp_linked::{
    LinkedSettings, PortaoDoCanal, admissao_vigente, aviso_portao_vazio, settings_from_config,
};

// Como `identidade_do_remetente` entrega: so digitos. A config aceita
// qualquer grafia (ver `chaves_de_users_casam_pela_chave_do_portao`).
const DONO: &str = "5511977776666";
const USUARIO: &str = "5511999998888";
const ESTRANHO: &str = "5521955554444";
/// Celular BR antigo, 12 digitos (sem o nono): a mesma pessoa que
/// `+55 31 99999-8888` (#1345).
const ANTIGO_12: &str = "553199998888";
const GRUPO: &str = "120363000000000000@g.us";

fn secao(settings: serde_json::Value) -> ChannelConfig {
    let mapa: HashMap<String, serde_json::Value> = settings
        .as_object()
        .expect("objeto")
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    ChannelConfig {
        channel_type: "whatsapp_linked".to_string(),
        enabled: Some(true),
        settings: mapa,
    }
}

fn config_com(settings: serde_json::Value) -> garraia_config::AppConfig {
    let mut config = garraia_config::AppConfig::default();
    config
        .channels
        .insert("whatsapp_linked".to_string(), secao(settings));
    config
}

fn legado() -> LinkedSettings {
    settings_from_config(&config_com(json!({
        "allow": [USUARIO],
        "owners": [DONO],
        "reply_in_groups": false,
    })))
}

// ---------------------------------------------------------------------------
// Compatibilidade: sem `access:` nada muda
// ---------------------------------------------------------------------------

#[test]
fn secao_sem_access_reproduz_o_legado() {
    let s = legado();
    let p = &s.access;
    assert_eq!(p.admission, Admission::Restricted);
    assert_eq!(p.default, Alcance::CHAT, "desconhecido nao ganha nada");
    assert!(!p.groups.enabled);
    assert_eq!(
        p.groups.default,
        Alcance::COMPLETO,
        "grupo sem politica declarada: sem teto, o piso de modo decide"
    );
    assert!(p.avisos.is_empty(), "{:?}", p.avisos);

    // `allow` = usuario sem teto (o piso de modo decide, como sempre);
    // `owners` = dono.
    let usuario = principal_do_turno(&s, USUARIO, "x@s.whatsapp.net", false, false);
    assert_eq!(usuario, Principal::Usuario(Alcance::COMPLETO));
    assert!(
        teto_do_principal(&usuario).is_none(),
        "legado nao ganha teto: nada muda para quem ja estava no allow"
    );
    let dono = principal_do_turno(&s, DONO, "x@s.whatsapp.net", false, false);
    assert_eq!(dono, Principal::Dono);
    let estranho = principal_do_turno(&s, ESTRANHO, "x@s.whatsapp.net", false, false);
    assert_eq!(estranho, Principal::Estranho);
    assert!(!estranho.admitido());
    assert_eq!(s.autorizados(), 2);
    assert_eq!(s.donos(), 1);
}

#[test]
fn read_declarado_ve_exatamente_o_que_o_search_ve_hoje() {
    // `level: read` sobre o piso `search` e um no-op: o teto NUNCA pode tirar
    // uma ferramenta que o `search` dava (regressao silenciosa) nem dar uma
    // que nao dava. E o que permite a CLI escrever `read` como default sem
    // mudar nada para quem migra.
    let s = settings_from_config(&config_com(json!({
        "access": { "users": { USUARIO: { "level": "read" } } }
    })));
    let principal = principal_do_turno(&s, USUARIO, "x@s.whatsapp.net", false, false);
    let teto = teto_do_principal(&principal);
    assert!(teto.is_some(), "usuario read declarado tem teto");
    let so_modo = ToolGate::for_mode_name("search");
    let com_teto = ToolGate::for_mode_name("search").com_teto(teto);
    for (nome, _) in NATIVAS {
        let caps = capacidades_nativas(nome);
        assert_eq!(
            so_modo.permite_com_capacidades(nome, caps),
            com_teto.permite_com_capacidades(nome, caps),
            "`{nome}`: o teto read mudou o que o search decide"
        );
    }
}

// ---------------------------------------------------------------------------
// `access:` — leitura, precedencia e fail-closed
// ---------------------------------------------------------------------------

#[test]
fn access_users_vence_o_legado_para_a_mesma_identidade() {
    let s = settings_from_config(&config_com(json!({
        "allow": [USUARIO],
        "owners": [DONO],
        "access": {
            "users": {
                USUARIO: { "level": "chat" },
                DONO: { "blocked": true },
            }
        }
    })));
    assert_eq!(
        principal_do_turno(&s, USUARIO, "x", false, false),
        Principal::Usuario(Alcance::CHAT)
    );
    assert_eq!(
        principal_do_turno(&s, DONO, "x", false, false),
        Principal::Bloqueado,
        "bloqueado em `access.users` vence `owners`"
    );
    assert_eq!(s.donos(), 0);
    assert_eq!(s.autorizados(), 1);
}

#[test]
fn chaves_de_users_casam_pela_chave_do_portao() {
    // A config tem o numero como o operador digita (13 digitos, com o 9); o
    // JID chega com 12. Mesma pessoa (#1345), mesma politica.
    let s = settings_from_config(&config_com(json!({
        "access": { "users": { "+55 31 99999-8888": { "level": "read", "write": true } } }
    })));
    assert_eq!(
        principal_do_turno(&s, ANTIGO_12, "x", false, false),
        Principal::Usuario(Alcance {
            nivel: Nivel::Read,
            write: true
        })
    );
    // E o portao admite pela mesma chave.
    assert!(PortaoDoCanal::from_settings(&s).libera(ANTIGO_12));
}

#[test]
fn admission_open_admite_desconhecido_com_o_default_e_nunca_full() {
    let aberto = settings_from_config(&config_com(json!({
        "access": { "admission": "open" }
    })));
    assert_eq!(aberto.access.admission, Admission::Open);
    assert_eq!(
        principal_do_turno(&aberto, ESTRANHO, "x", false, false),
        Principal::Desconhecido(Alcance::CHAT),
        "sem `default`, desconhecido so conversa"
    );
    assert!(PortaoDoCanal::from_settings(&aberto).libera(ESTRANHO));

    // `default: full` e recusado: cai em read, com aviso (#1390).
    let full = settings_from_config(&config_com(json!({
        "access": { "admission": "open", "default": { "level": "full", "write": true } }
    })));
    assert_eq!(
        principal_do_turno(&full, ESTRANHO, "x", false, false),
        Principal::Desconhecido(Alcance {
            nivel: Nivel::Read,
            write: true
        })
    );
    assert!(
        full.access.avisos.iter().any(|a| a.contains("default")),
        "{:?}",
        full.access.avisos
    );

    // `admission` invalido e restricted, com aviso.
    let invalido = settings_from_config(&config_com(json!({
        "access": { "admission": "aberto" }
    })));
    assert_eq!(invalido.access.admission, Admission::Restricted);
    assert!(!PortaoDoCanal::from_settings(&invalido).libera(ESTRANHO));
    assert_eq!(
        invalido.access.avisos.len(),
        1,
        "{:?}",
        invalido.access.avisos
    );
}

#[test]
fn bloqueado_vence_open_allow_e_pareamento() {
    let s = settings_from_config(&config_com(json!({
        "allow": [ESTRANHO],
        "access": { "admission": "open", "users": { ESTRANHO: { "blocked": true } } }
    })));
    let portao = PortaoDoCanal::from_settings(&s);
    assert!(
        !portao.libera(ESTRANHO),
        "bloqueado nao entra nem em `open`"
    );
    assert!(
        portao.libera(USUARIO),
        "`open` continua valendo para os outros"
    );
    assert_eq!(
        principal_do_turno(&s, ESTRANHO, "x", false, true),
        Principal::Bloqueado,
        "pareamento nao desbloqueia"
    );
}

#[test]
fn nivel_invalido_e_chat_com_write_caem_em_chat_com_aviso() {
    let s = settings_from_config(&config_com(json!({
        "access": { "users": {
            USUARIO: { "level": "pesquisa", "write": true },
            DONO: { "level": "chat", "write": true },
        } }
    })));
    assert_eq!(
        principal_do_turno(&s, USUARIO, "x", false, false),
        Principal::Usuario(Alcance::CHAT),
        "nivel desconhecido e fail-closed"
    );
    assert_eq!(
        principal_do_turno(&s, DONO, "x", false, false),
        Principal::Usuario(Alcance::CHAT),
        "`chat` com `write: true` normaliza para chat"
    );
    assert_eq!(s.access.avisos.len(), 2, "{:?}", s.access.avisos);
    for aviso in &s.access.avisos {
        assert!(!aviso.contains("9999"), "aviso carrega numero: {aviso}");
        assert!(!aviso.contains("7777"), "aviso carrega numero: {aviso}");
    }
}

#[test]
fn nivel_pode_vir_como_string_curta() {
    let s = settings_from_config(&config_com(json!({
        "access": { "users": { USUARIO: "read" } }
    })));
    assert_eq!(
        principal_do_turno(&s, USUARIO, "x", false, false),
        Principal::Usuario(Alcance::LEITURA)
    );
}

#[test]
fn role_owner_em_access_users_e_dono() {
    let s = settings_from_config(&config_com(json!({
        "access": { "users": { DONO: { "role": "owner" } } }
    })));
    assert_eq!(
        principal_do_turno(&s, DONO, "x", false, false),
        Principal::Dono
    );
    assert!(s.e_dono(DONO));
    assert_eq!(s.donos(), 1);
    assert!(PortaoDoCanal::from_settings(&s).libera(DONO));
}

// ---------------------------------------------------------------------------
// Grupos
// ---------------------------------------------------------------------------

#[test]
fn grupo_nunca_herda_dono_e_tem_politica_propria() {
    let s = settings_from_config(&config_com(json!({
        "owners": [DONO],
        "access": { "groups": {
            "enabled": true,
            "default": { "level": "chat" },
            GRUPO: { "level": "read" },
            "outro@g.us": { "level": "full", "write": true },
        } }
    })));
    assert!(s.access.groups.enabled);
    assert!(s.responde_em_grupo());
    // O dono falando no grupo e o grupo, nao o dono.
    assert_eq!(
        principal_do_turno(&s, DONO, GRUPO, true, false),
        Principal::Grupo(Alcance::LEITURA)
    );
    // Grupo sem entrada propria recebe o default dos grupos.
    assert_eq!(
        principal_do_turno(&s, DONO, "qualquer@g.us", true, false),
        Principal::Grupo(Alcance::CHAT)
    );
    // Quem ninguem conhece continua fora, tambem em grupo (a admissao e
    // `restricted`): a politica do grupo e para quem entra.
    assert_eq!(
        principal_do_turno(&s, USUARIO, "qualquer@g.us", true, false),
        Principal::Estranho
    );
    // `full` declarado num grupo e honrado (sem teto) — mas continua sendo o
    // GRUPO, com o piso de grupo: nunca o perfil do dono.
    let outro = principal_do_turno(&s, DONO, "outro@g.us", true, false);
    assert_eq!(outro, Principal::Grupo(Alcance::COMPLETO));
    assert!(teto_do_principal(&outro).is_none());
    assert!(s.access.avisos.is_empty(), "{:?}", s.access.avisos);
    // Bloqueado continua bloqueado dentro do grupo.
    let bloq = settings_from_config(&config_com(json!({
        "reply_in_groups": true,
        "access": { "users": { USUARIO: { "blocked": true } } }
    })));
    assert_eq!(
        principal_do_turno(&bloq, USUARIO, GRUPO, true, false),
        Principal::Bloqueado
    );
}

#[test]
fn reply_in_groups_legado_continua_ligando_grupos() {
    let s = settings_from_config(&config_com(json!({ "reply_in_groups": true })));
    assert!(s.responde_em_grupo());
    assert!(s.access.groups.enabled);
}

// ---------------------------------------------------------------------------
// Teto por principal
// ---------------------------------------------------------------------------

fn portao_aberto_com(principal: &Principal) -> ToolGate {
    // Sem modo: so o teto decide — e o que isola o que o teto faz.
    ToolGate::sem_politica().com_teto(teto_do_principal(principal))
}

fn permite(gate: &ToolGate, nome: &str) -> bool {
    gate.permite_com_capacidades(nome, capacidades_nativas(nome))
}

#[test]
fn teto_do_principal_por_nivel_e_write() {
    let dono = portao_aberto_com(&Principal::Dono);
    assert!(permite(&dono, "bash") && permite(&dono, "file_write"));
    assert!(
        teto_do_principal(&Principal::Dono).is_none(),
        "dono nao tem teto: quem decide e o modo e o perfil de execucao"
    );

    let leitura = portao_aberto_com(&Principal::Usuario(Alcance::LEITURA));
    assert!(permite(&leitura, "file_read") && permite(&leitura, "web_search"));
    assert!(!permite(&leitura, "file_write"), "write off");
    assert!(!permite(&leitura, "bash"), "read nunca executa");
    assert!(!permite(&leitura, "telegram_send"));
    assert!(!permite(&leitura, "device_execute"));

    let escrita = portao_aberto_com(&Principal::Usuario(Alcance {
        nivel: Nivel::Read,
        write: true,
    }));
    assert!(
        permite(&escrita, "file_write"),
        "write on libera escrita de arquivo"
    );
    assert!(!permite(&escrita, "bash"), "write on NAO libera shell");
    assert!(!permite(&escrita, "device_execute"), "nem dispositivo");

    let chat = portao_aberto_com(&Principal::Desconhecido(Alcance::CHAT));
    for (nome, _) in NATIVAS {
        assert!(!permite(&chat, nome), "`chat` nao tem ferramenta: {nome}");
    }

    let pareado = portao_aberto_com(&Principal::Pareado);
    assert!(permite(&pareado, "file_read") && !permite(&pareado, "file_write"));

    // Um call-site que passe um nao-admitido nao pode abrir nada.
    for p in [Principal::Bloqueado, Principal::Estranho] {
        let g = portao_aberto_com(&p);
        assert!(!permite(&g, "file_read"), "{p:?} com ferramenta");
    }
}

#[test]
fn a_recusa_do_teto_nomeia_a_politica_e_nao_o_modo() {
    let principal = Principal::Usuario(Alcance::LEITURA);
    let gate = ToolGate::for_mode_name("code").com_teto(teto_do_principal(&principal));
    assert!(!permite(&gate, "bash"));
    let recusa = gate.explica_recusa("bash", capacidades_nativas("bash"));
    assert!(recusa.contains("politica de acesso"), "{recusa}");
    assert!(recusa.contains("read"), "{recusa}");
    assert!(recusa.contains("write off"), "{recusa}");
}

// ---------------------------------------------------------------------------
// Hot reload e superficies que contam
// ---------------------------------------------------------------------------

#[test]
fn admissao_vigente_recarrega_a_politica_inteira() {
    let boot = legado();
    let viva = settings_from_config(&config_com(json!({
        "access": { "admission": "open", "users": { USUARIO: { "level": "read", "write": true } } }
    })));
    let vigente = admissao_vigente(&boot, &viva);
    assert_eq!(vigente.access.admission, Admission::Open);
    assert_eq!(
        principal_do_turno(&vigente, USUARIO, "x", false, false),
        Principal::Usuario(Alcance {
            nivel: Nivel::Read,
            write: true
        })
    );
    assert_eq!(
        principal_do_turno(&vigente, DONO, "x", false, false),
        Principal::Desconhecido(Alcance::CHAT),
        "dono que saiu da config viva perde o papel na mensagem seguinte"
    );
    // Canal desligado na config viva: politica fail-closed (ninguem).
    let mut desligada = viva.clone();
    desligada.enabled = false;
    let vigente = admissao_vigente(&boot, &desligada);
    assert!(!vigente.enabled);
    assert_eq!(vigente.access, PoliticaDeAcesso::default());
    assert!(!PortaoDoCanal::from_settings(&vigente).libera(USUARIO));
}

#[test]
fn portao_recarregado_esquece_quem_saiu_e_bloqueia_quem_foi_bloqueado() {
    let boot = legado();
    let mut portao = PortaoDoCanal::from_settings(&boot);
    assert!(portao.libera(USUARIO));
    let viva = settings_from_config(&config_com(json!({
        "allow": [USUARIO],
        "access": { "users": { USUARIO: { "blocked": true } } }
    })));
    portao.recarregar(&viva);
    assert!(
        !portao.libera(USUARIO),
        "bloqueio a quente vale na proxima mensagem"
    );
    assert!(
        !portao.libera(DONO),
        "dono que saiu da config saiu do portao"
    );
}

#[test]
fn aviso_de_portao_vazio_respeita_open_e_conta_access_users() {
    let vazio = settings_from_config(&config_com(json!({})));
    assert!(aviso_portao_vazio(&vazio, "garraia", true).is_some());
    let aberto = settings_from_config(&config_com(json!({ "access": { "admission": "open" } })));
    assert!(
        aviso_portao_vazio(&aberto, "garraia", true).is_none(),
        "`open` nao e portao vazio"
    );
    let so_access = settings_from_config(&config_com(json!({
        "access": { "users": { USUARIO: { "level": "read" } } }
    })));
    assert_eq!(so_access.autorizados(), 1);
    assert!(aviso_portao_vazio(&so_access, "garraia", true).is_none());
}

#[test]
fn principal_tem_etiqueta_sem_identidade() {
    for (p, esperado) in [
        (Principal::Dono, "dono"),
        (Principal::Usuario(Alcance::LEITURA), "usuario"),
        (Principal::Pareado, "pareado"),
        (Principal::Desconhecido(Alcance::CHAT), "desconhecido"),
        (Principal::Grupo(Alcance::LEITURA), "grupo"),
        (Principal::Bloqueado, "bloqueado"),
        (Principal::Estranho, "estranho"),
    ] {
        assert_eq!(p.as_str(), esperado);
    }
    assert_eq!(Alcance::LEITURA.to_string(), "read, write off");
    assert_eq!(
        Alcance {
            nivel: Nivel::Full,
            write: true
        }
        .to_string(),
        "full, write on"
    );
}
