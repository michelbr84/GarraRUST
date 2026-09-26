//! O caminho UNICO de mutacao da Access Policy v2 (#1412, #1413, #1414):
//! `mutacao::aplicar` sobre a secao, `impacto::matriz`/`impacto::diferencas`
//! pelo mesmo motor do turno, e `auditoria` sem identidade nem segredo. CLI,
//! API admin e Web Console chamam isto — nunca reimplementam.

use std::collections::HashMap;

use garraia_agents::modes::Nivel;
use garraia_config::{ChannelConfig, ExecutionProfile};
use serde_json::json;

use crate::bootstrap::whatsapp_linked::politica::auditoria::{self, Evento};
use crate::bootstrap::whatsapp_linked::politica::impacto::{self, Efetivo};
use crate::bootstrap::whatsapp_linked::politica::mutacao::{self, Mutacao, MutacaoInvalida};
use crate::bootstrap::whatsapp_linked::politica::{Admission, Alcance, Principal};
use crate::bootstrap::whatsapp_linked::{LinkedSettings, settings_from_config};

const DONO: &str = "5511977776666";
const USUARIO: &str = "5511999998888";
const OUTRO: &str = "5521955554444";
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

fn settings_de(secao: &ChannelConfig) -> LinkedSettings {
    let mut config = garraia_config::AppConfig::default();
    config
        .channels
        .insert("whatsapp_linked".to_string(), secao.clone());
    settings_from_config(&config)
}

fn legado() -> ChannelConfig {
    secao(json!({ "allow": [USUARIO], "owners": [DONO], "reply_in_groups": false }))
}

fn principal(secao: &ChannelConfig, quem: &str) -> Principal {
    crate::bootstrap::whatsapp_linked::principal_do_turno(
        &settings_de(secao),
        quem,
        "x",
        false,
        false,
    )
}

// ---------------------------------------------------------------------------
// mutacao::aplicar
// ---------------------------------------------------------------------------

#[test]
fn nivel_e_write_gravam_em_access_users_e_vencem_o_legado() {
    let mut s = legado();
    let a = mutacao::aplicar(
        &mut s,
        &Mutacao::Nivel {
            identidade: "+55 11 99999-8888".to_string(),
            nivel: Nivel::Read,
        },
    )
    .expect("aplica");
    assert!(a.mudou);
    assert_eq!(
        principal(&s, USUARIO),
        Principal::Usuario(Alcance::LEITURA),
        "o legado (sem teto) virou read declarado"
    );
    // A chave gravada e a identidade normalizada, nao a grafia digitada; e
    // repetir com outra grafia nao duplica.
    let users = s.settings["access"]["users"].as_object().expect("mapa");
    assert_eq!(users.len(), 1);
    assert!(users.contains_key(USUARIO), "{users:?}");

    let b = mutacao::aplicar(
        &mut s,
        &Mutacao::Write {
            identidade: USUARIO.to_string(),
            on: true,
        },
    )
    .expect("aplica");
    assert!(b.mudou);
    assert_eq!(
        principal(&s, USUARIO),
        Principal::Usuario(Alcance {
            nivel: Nivel::Read,
            write: true
        })
    );
    // Idempotente: a mesma mutacao de novo nao muda nada.
    let c = mutacao::aplicar(
        &mut s,
        &Mutacao::Write {
            identidade: USUARIO.to_string(),
            on: true,
        },
    )
    .expect("aplica");
    assert!(!c.mudou);
    assert_eq!(c.antes, c.depois);
}

#[test]
fn write_off_num_legado_vira_full_sem_escrita() {
    // `allow` legado nao tem teto; `write off` precisa TIRAR so a escrita de
    // arquivo — entao a entrada nasce `full, write off`, e nao `read`.
    let mut s = legado();
    mutacao::aplicar(
        &mut s,
        &Mutacao::Write {
            identidade: USUARIO.to_string(),
            on: false,
        },
    )
    .expect("aplica");
    assert_eq!(
        principal(&s, USUARIO),
        Principal::Usuario(Alcance {
            nivel: Nivel::Full,
            write: false
        })
    );
}

#[test]
fn combinacoes_invalidas_sao_recusadas_com_erro_acionavel() {
    let mut s = legado();
    // `default: full` para desconhecido (#1390).
    let e = mutacao::aplicar(&mut s, &Mutacao::DefaultDesconhecido(Alcance::COMPLETO)).unwrap_err();
    assert!(matches!(e, MutacaoInvalida::DefaultFull), "{e:?}");
    assert!(e.to_string().contains("read"), "diz o que vale: {e}");
    // `chat` com write.
    let e = mutacao::aplicar(
        &mut s,
        &Mutacao::DefaultDesconhecido(Alcance {
            nivel: Nivel::Chat,
            write: true,
        }),
    )
    .unwrap_err();
    assert!(matches!(e, MutacaoInvalida::ChatComWrite));
    // write em quem ninguem conhece.
    let e = mutacao::aplicar(
        &mut s,
        &Mutacao::Write {
            identidade: OUTRO.to_string(),
            on: true,
        },
    )
    .unwrap_err();
    assert!(matches!(e, MutacaoInvalida::Desconhecida), "{e:?}");
    assert!(
        e.to_string().contains("allow") || e.to_string().contains("level"),
        "{e}"
    );
    // nivel/write no dono: o dono nao tem teto; mexer nele e `unowner`.
    let e = mutacao::aplicar(
        &mut s,
        &Mutacao::Nivel {
            identidade: DONO.to_string(),
            nivel: Nivel::Read,
        },
    )
    .unwrap_err();
    assert!(matches!(e, MutacaoInvalida::EDono), "{e:?}");
    assert!(e.to_string().contains("unowner"), "{e}");
    // identidade vazia.
    let e = mutacao::aplicar(
        &mut s,
        &Mutacao::Nivel {
            identidade: "  ".to_string(),
            nivel: Nivel::Read,
        },
    )
    .unwrap_err();
    assert!(matches!(e, MutacaoInvalida::IdentidadeVazia));
    // secao de outro canal: nao escreve.
    let mut outra = legado();
    outra.channel_type = "whatsapp".to_string();
    let e = mutacao::aplicar(&mut outra, &Mutacao::Admissao(Admission::Open)).unwrap_err();
    assert!(matches!(e, MutacaoInvalida::SecaoDeOutroCanal(_)));
    // Nada mudou na secao original depois das recusas.
    assert_eq!(s.settings, legado().settings);
}

#[test]
fn chat_apaga_o_write_que_havia() {
    let mut s =
        secao(json!({ "access": { "users": { USUARIO: { "level": "read", "write": true } } } }));
    mutacao::aplicar(
        &mut s,
        &Mutacao::Nivel {
            identidade: USUARIO.to_string(),
            nivel: Nivel::Chat,
        },
    )
    .expect("aplica");
    assert_eq!(principal(&s, USUARIO), Principal::Usuario(Alcance::CHAT));
    assert_eq!(
        s.settings["access"]["users"][USUARIO]["write"],
        json!(false)
    );
}

#[test]
fn bloquear_e_desbloquear_preservam_o_resto_da_entrada() {
    let mut s = secao(
        json!({ "allow": [USUARIO], "access": { "users": { USUARIO: { "level": "read" } } } }),
    );
    let a = mutacao::aplicar(&mut s, &Mutacao::Bloquear(USUARIO.to_string())).expect("aplica");
    assert!(a.mudou);
    assert_eq!(principal(&s, USUARIO), Principal::Bloqueado);
    assert_eq!(
        s.settings["access"]["users"][USUARIO]["level"],
        json!("read"),
        "o nivel fica"
    );
    let b = mutacao::aplicar(&mut s, &Mutacao::Desbloquear(USUARIO.to_string())).expect("aplica");
    assert!(b.mudou);
    assert_eq!(principal(&s, USUARIO), Principal::Usuario(Alcance::LEITURA));
    assert!(
        s.settings["access"]["users"][USUARIO]
            .get("blocked")
            .is_none()
    );
    // Bloquear quem ninguem conhece cria a entrada so com `blocked`; e
    // desbloquear depois a remove inteira (volta a ser ninguem).
    mutacao::aplicar(&mut s, &Mutacao::Bloquear(OUTRO.to_string())).expect("aplica");
    assert_eq!(principal(&s, OUTRO), Principal::Bloqueado);
    mutacao::aplicar(&mut s, &Mutacao::Desbloquear(OUTRO.to_string())).expect("aplica");
    assert_eq!(principal(&s, OUTRO), Principal::Estranho);
    assert!(s.settings["access"]["users"].get(OUTRO).is_none());
}

#[test]
fn admissao_e_gravada_explicita_nas_duas_direcoes() {
    let mut s = legado();
    mutacao::aplicar(&mut s, &Mutacao::Admissao(Admission::Open)).expect("aplica");
    assert_eq!(s.settings["access"]["admission"], json!("open"));
    assert_eq!(principal(&s, OUTRO), Principal::Desconhecido(Alcance::CHAT));
    // Voltar grava `restricted` por extenso: politica declarada, nunca
    // inferida da ausencia (#1388).
    mutacao::aplicar(&mut s, &Mutacao::Admissao(Admission::Restricted)).expect("aplica");
    assert_eq!(s.settings["access"]["admission"], json!("restricted"));
    assert_eq!(principal(&s, OUTRO), Principal::Estranho);
    // Trocar admissao preserva os usuarios explicitos (#1396).
    assert_eq!(
        principal(&s, USUARIO),
        Principal::Usuario(Alcance::COMPLETO)
    );
    assert_eq!(principal(&s, DONO), Principal::Dono);
}

#[test]
fn default_do_desconhecido_fica_guardado_em_restricted_ate_reabrir() {
    let mut s = legado();
    mutacao::aplicar(&mut s, &Mutacao::DefaultDesconhecido(Alcance::LEITURA)).expect("aplica");
    assert_eq!(
        principal(&s, OUTRO),
        Principal::Estranho,
        "restricted nao aplica o default"
    );
    mutacao::aplicar(&mut s, &Mutacao::Admissao(Admission::Open)).expect("aplica");
    assert_eq!(
        principal(&s, OUTRO),
        Principal::Desconhecido(Alcance::LEITURA)
    );
}

#[test]
fn grupos_ligam_e_desligam_tambem_o_reply_in_groups_legado() {
    // O dono declarado: em grupo ele e o grupo, mas quem ninguem conhece nao
    // entra (a admissao e `restricted`).
    let mut s = secao(json!({ "reply_in_groups": true, "owners": [DONO] }));
    assert!(settings_de(&s).responde_em_grupo());
    mutacao::aplicar(&mut s, &Mutacao::Grupos(false)).expect("aplica");
    assert!(
        !settings_de(&s).responde_em_grupo(),
        "desligar tem de desligar de verdade"
    );
    assert_eq!(s.settings["reply_in_groups"], json!(false));
    mutacao::aplicar(&mut s, &Mutacao::Grupos(true)).expect("aplica");
    assert!(settings_de(&s).responde_em_grupo());
    mutacao::aplicar(
        &mut s,
        &Mutacao::Grupo {
            jid: GRUPO.to_string(),
            alcance: Alcance::CHAT,
        },
    )
    .expect("aplica");
    mutacao::aplicar(&mut s, &Mutacao::DefaultDeGrupo(Alcance::LEITURA)).expect("aplica");
    let st = settings_de(&s);
    assert_eq!(
        crate::bootstrap::whatsapp_linked::principal_do_turno(&st, DONO, GRUPO, true, false),
        Principal::Grupo(Alcance::CHAT)
    );
    assert_eq!(
        crate::bootstrap::whatsapp_linked::principal_do_turno(&st, DONO, "outro@g.us", true, false),
        Principal::Grupo(Alcance::LEITURA)
    );
    let e = mutacao::aplicar(
        &mut s,
        &Mutacao::Grupo {
            jid: "  ".to_string(),
            alcance: Alcance::CHAT,
        },
    )
    .unwrap_err();
    assert!(matches!(e, MutacaoInvalida::IdentidadeVazia));
}

#[test]
fn reset_volta_ao_seguro_preserva_donos_e_bloqueios_e_e_idempotente() {
    let mut s = secao(json!({
        "allow": [USUARIO],
        "owners": [DONO],
        "reply_in_groups": true,
        "access": {
            "admission": "open",
            "default": { "level": "read", "write": true },
            "users": {
                USUARIO: { "level": "full", "write": true },
                OUTRO: { "blocked": true },
                "5531988887777": { "role": "owner" },
            },
            "groups": { "enabled": true, "default": { "level": "read" }, GRUPO: { "level": "read", "write": true } }
        }
    }));
    let a = mutacao::aplicar(&mut s, &Mutacao::Reset).expect("aplica");
    assert!(a.mudou);
    let st = settings_de(&s);
    assert_eq!(st.access.admission, Admission::Restricted);
    assert_eq!(st.access.default, Alcance::CHAT);
    assert!(!st.responde_em_grupo());
    assert!(st.access.groups.por_grupo.is_empty());
    assert_eq!(
        principal(&s, USUARIO),
        Principal::Usuario(Alcance::COMPLETO),
        "override saiu; o allow legado fica"
    );
    assert_eq!(principal(&s, OUTRO), Principal::Bloqueado, "bloqueio fica");
    assert_eq!(principal(&s, DONO), Principal::Dono, "dono legado fica");
    assert_eq!(
        principal(&s, "5531988887777"),
        Principal::Dono,
        "role: owner fica"
    );
    // O que mudou e nomeado, sem identidade.
    assert!(!a.mudancas.is_empty());
    for m in &a.mudancas {
        assert!(
            !m.contains("9999") && !m.contains("5554"),
            "mudanca com numero: {m}"
        );
    }
    // Idempotente.
    let b = mutacao::aplicar(&mut s, &Mutacao::Reset).expect("aplica");
    assert!(!b.mudou);
    assert!(b.mudancas.is_empty());
}

// ---------------------------------------------------------------------------
// impacto — o mesmo motor do turno
// ---------------------------------------------------------------------------

fn efetivo_de(secao: &ChannelConfig, quem: &str) -> Efetivo {
    let st = settings_de(secao);
    let p = crate::bootstrap::whatsapp_linked::principal_do_turno(&st, quem, "x", false, false);
    impacto::efetivo(&st, ExecutionProfile::Standard, p, false)
}

#[test]
fn efetivo_reflete_piso_e_teto_pelo_tool_gate_real() {
    let s = legado();
    let usuario = efetivo_de(&s, USUARIO);
    assert_eq!(usuario.modo, "search", "piso legado");
    assert!(usuario.capacidades.leitura);
    assert!(
        !usuario.capacidades.escrita,
        "`search` nao escreve, com ou sem teto"
    );
    assert!(!usuario.capacidades.shell);

    let mut com_write = legado();
    mutacao::aplicar(
        &mut com_write,
        &Mutacao::Nivel {
            identidade: USUARIO.to_string(),
            nivel: Nivel::Read,
        },
    )
    .expect("aplica");
    mutacao::aplicar(
        &mut com_write,
        &Mutacao::Write {
            identidade: USUARIO.to_string(),
            on: true,
        },
    )
    .expect("aplica");
    let e = efetivo_de(&com_write, USUARIO);
    assert!(
        !e.capacidades.escrita,
        "write on nao vence o piso `search`: o teto so tira"
    );
    assert!(!e.capacidades.shell && !e.capacidades.dispositivo && !e.capacidades.mensagem);

    // Dono em pod: piso `code`, sem teto — e o `write off` declarado num
    // usuario `full` tira SO a escrita (nativa e MCP), nunca o shell.
    let mut pod = secao(json!({ "default_mode": "code", "allow": [USUARIO] }));
    let st = settings_de(&pod);
    let livre = impacto::efetivo(
        &st,
        ExecutionProfile::Standard,
        principal(&pod, USUARIO),
        false,
    );
    assert!(
        livre.capacidades.escrita && livre.capacidades.mcp_escrita,
        "legado sem teto no piso code"
    );
    mutacao::aplicar(
        &mut pod,
        &Mutacao::Write {
            identidade: USUARIO.to_string(),
            on: false,
        },
    )
    .expect("aplica");
    let st = settings_de(&pod);
    let sem_escrita = impacto::efetivo(
        &st,
        ExecutionProfile::Standard,
        principal(&pod, USUARIO),
        false,
    );
    assert!(
        !sem_escrita.capacidades.escrita,
        "write off nega file_write"
    );
    assert!(
        !sem_escrita.capacidades.mcp_escrita,
        "write off nega MCP write"
    );
    assert_eq!(
        sem_escrita.capacidades.shell, livre.capacidades.shell,
        "shell e controle independente"
    );
    assert!(sem_escrita.capacidades.leitura && sem_escrita.capacidades.mcp_leitura);
}

#[test]
fn matriz_lista_todo_principal_sem_identidade_inteira() {
    let s = secao(json!({
        "allow": [USUARIO], "owners": [DONO],
        "access": { "admission": "open", "users": { OUTRO: { "blocked": true } }, "groups": { "enabled": true, GRUPO: { "level": "chat" } } }
    }));
    let linhas = impacto::matriz(&settings_de(&s), ExecutionProfile::IsolatedPod);
    let etiquetas: Vec<&str> = linhas.iter().map(|l| l.principal).collect();
    assert!(etiquetas.contains(&"dono"), "{etiquetas:?}");
    assert!(etiquetas.contains(&"usuario"));
    assert!(etiquetas.contains(&"bloqueado"));
    assert!(
        etiquetas.contains(&"desconhecido"),
        "open lista o desconhecido"
    );
    assert!(etiquetas.contains(&"pareado"));
    assert!(etiquetas.contains(&"grupo"));
    for l in &linhas {
        let texto = format!("{l:?}");
        for numero in [USUARIO, DONO, OUTRO, "120363000000000000"] {
            assert!(!texto.contains(numero), "matriz vaza identidade: {texto}");
        }
    }
    let dono = linhas.iter().find(|l| l.principal == "dono").expect("dono");
    assert_eq!(dono.efetivo.modo, "code", "dono em pod");
    let grupo = linhas
        .iter()
        .find(|l| l.principal == "grupo" && l.alvo.is_some())
        .expect("grupo com jid");
    assert_eq!(grupo.efetivo.alcance, Some(Alcance::CHAT));
    // Sem `open`, o desconhecido nao aparece: ele nao entra.
    let fechado = impacto::matriz(&settings_de(&legado()), ExecutionProfile::Standard);
    assert!(fechado.iter().all(|l| l.principal != "desconhecido"));
}

#[test]
fn diferencas_nomeiam_o_que_cada_principal_ganha_e_perde() {
    let antes = secao(json!({ "default_mode": "code", "allow": [USUARIO] }));
    let mut depois = antes.clone();
    mutacao::aplicar(
        &mut depois,
        &Mutacao::Write {
            identidade: USUARIO.to_string(),
            on: false,
        },
    )
    .expect("aplica");
    let difs = impacto::diferencas(
        &settings_de(&antes),
        &settings_de(&depois),
        ExecutionProfile::Standard,
    );
    assert_eq!(difs.len(), 1, "{difs:?}");
    let d = &difs[0];
    assert_eq!(d.principal, "usuario");
    assert!(d.perde.iter().any(|c| c.contains("escrita")), "{d:?}");
    assert!(d.perde.iter().any(|c| c.contains("MCP")), "{d:?}");
    assert!(d.ganha.is_empty());
    assert!(!format!("{d:?}").contains(USUARIO));
    // Sem mudanca efetiva, sem diferenca — mesmo que a config tenha mudado
    // (declarar `read` sobre o piso `search` e um no-op).
    let mut so_read = legado();
    mutacao::aplicar(
        &mut so_read,
        &Mutacao::Nivel {
            identidade: USUARIO.to_string(),
            nivel: Nivel::Read,
        },
    )
    .expect("aplica");
    assert!(
        impacto::diferencas(
            &settings_de(&legado()),
            &settings_de(&so_read),
            ExecutionProfile::Standard
        )
        .is_empty()
    );
    // Abrir a admissao faz o desconhecido APARECER (ganha "conversar").
    let mut aberto = legado();
    mutacao::aplicar(&mut aberto, &Mutacao::Admissao(Admission::Open)).expect("aplica");
    let difs = impacto::diferencas(
        &settings_de(&legado()),
        &settings_de(&aberto),
        ExecutionProfile::Standard,
    );
    assert!(
        difs.iter()
            .any(|d| d.principal == "desconhecido" && !d.ganha.is_empty()),
        "{difs:?}"
    );
}

// ---------------------------------------------------------------------------
// auditoria
// ---------------------------------------------------------------------------

#[test]
fn auditoria_grava_jsonl_sem_identidade_le_de_tras_para_frente_e_rotaciona() {
    let dir = tempfile::tempdir().expect("tempdir");
    let antes = settings_de(&legado());
    let mut s = legado();
    mutacao::aplicar(&mut s, &Mutacao::Bloquear(USUARIO.to_string())).expect("aplica");
    let depois = settings_de(&s);
    let evento = Evento::novo("cli", "michel", "block", Some(USUARIO), &antes, &depois);
    assert!(evento.ts.ends_with('Z'), "UTC explicito: {}", evento.ts);
    assert_eq!(
        evento.alvo.as_deref(),
        Some("…8888"),
        "so os quatro ultimos"
    );
    let caminho = auditoria::registrar(dir.path(), &evento, 1 << 20).expect("grava");
    assert!(caminho.ends_with("audit/whatsapp-access.jsonl"));
    let bruto = std::fs::read_to_string(&caminho).expect("le");
    assert!(
        !bruto.contains(USUARIO) && !bruto.contains(DONO),
        "audit com numero: {bruto}"
    );
    assert!(bruto.contains("\"block\""));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let modo = std::fs::metadata(&caminho)
            .expect("meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(modo, 0o600, "0600, como o config.yml");
    }
    for i in 0..3 {
        let e = Evento::novo("admin_api", "admin", "level", Some(OUTRO), &antes, &depois);
        let _ = i;
        auditoria::registrar(dir.path(), &e, 1 << 20).expect("grava");
    }
    let lidos = auditoria::ler(dir.path(), 2).expect("le");
    assert_eq!(lidos.len(), 2, "limite");
    assert_eq!(lidos[0].acao, "level", "mais recente primeiro");
    let todos = auditoria::ler(dir.path(), 100).expect("le");
    assert_eq!(todos.len(), 4);
    assert_eq!(todos[3].acao, "block");
    // Rotacao por tamanho: com o teto minusculo, o arquivo vira `.1` e o novo
    // comeca do zero — nunca cresce sem limite.
    auditoria::registrar(dir.path(), &evento, 10).expect("grava");
    assert!(dir.path().join("audit/whatsapp-access.jsonl.1").exists());
    let novo = std::fs::read_to_string(&caminho).expect("le");
    assert_eq!(novo.lines().count(), 1);
    // Sem diretorio de audit ainda: `ler` devolve vazio, nao erro.
    let vazio = tempfile::tempdir().expect("tempdir");
    assert!(auditoria::ler(vazio.path(), 10).expect("le").is_empty());
}

#[test]
fn auditoria_falha_de_forma_segura_quando_nao_consegue_gravar() {
    // Um arquivo no lugar do diretorio `audit/`: `registrar` devolve Err (e o
    // chamador decide o que dizer) em vez de entrar em panico ou fingir que
    // gravou.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("audit"), b"nao sou diretorio").expect("escreve");
    let antes = settings_de(&legado());
    let evento = Evento::novo("cli", "x", "open", None, &antes, &antes);
    assert!(auditoria::registrar(dir.path(), &evento, 1 << 20).is_err());
}

// ---------------------------------------------------------------------------
// Papel (dono) e remocao — as acoes que faltavam ao console (#1404)
// ---------------------------------------------------------------------------

#[test]
fn papel_de_dono_liga_e_desliga_preservando_o_acesso() {
    let mut s = legado();
    // Promover quem esta no `allow`: `role: owner` em access.users.
    let a = mutacao::aplicar(
        &mut s,
        &Mutacao::Papel {
            identidade: USUARIO.to_string(),
            dono: true,
        },
    )
    .expect("aplica");
    assert!(a.mudou);
    assert_eq!(principal(&s, USUARIO), Principal::Dono);
    assert_eq!(
        s.settings["access"]["users"][USUARIO]["role"],
        json!("owner")
    );
    // Rebaixar o dono LEGADO (so em `owners`): sai de `owners`, entra em
    // `allow` — o acesso e preservado, como `garraia whatsapp unowner` faz.
    let b = mutacao::aplicar(
        &mut s,
        &Mutacao::Papel {
            identidade: DONO.to_string(),
            dono: false,
        },
    )
    .expect("aplica");
    assert!(b.mudou);
    assert_eq!(
        principal(&s, DONO),
        Principal::Usuario(Alcance::COMPLETO),
        "acesso preservado"
    );
    let owners = s.settings["owners"].as_array().cloned().unwrap_or_default();
    assert!(owners.is_empty(), "{owners:?}");
    // Rebaixar quem virou dono por `role`: a chave some, o resto da entrada fica.
    mutacao::aplicar(
        &mut s,
        &Mutacao::Nivel {
            identidade: USUARIO.to_string(),
            nivel: Nivel::Read,
        },
    )
    .unwrap_err();
    let c = mutacao::aplicar(
        &mut s,
        &Mutacao::Papel {
            identidade: USUARIO.to_string(),
            dono: false,
        },
    )
    .expect("aplica");
    assert!(c.mudou);
    assert_eq!(
        principal(&s, USUARIO),
        Principal::Usuario(Alcance::COMPLETO)
    );
    assert!(s.settings["access"]["users"][USUARIO].get("role").is_none());
    // Idempotente nas duas direcoes; bloqueado nao vira dono.
    assert!(
        !mutacao::aplicar(
            &mut s,
            &Mutacao::Papel {
                identidade: USUARIO.to_string(),
                dono: false
            }
        )
        .expect("aplica")
        .mudou
    );
    mutacao::aplicar(&mut s, &Mutacao::Bloquear(OUTRO.to_string())).expect("aplica");
    let e = mutacao::aplicar(
        &mut s,
        &Mutacao::Papel {
            identidade: OUTRO.to_string(),
            dono: true,
        },
    )
    .unwrap_err();
    assert!(matches!(e, MutacaoInvalida::Bloqueada), "{e:?}");
    assert!(e.to_string().contains("unblock"), "{e}");
    for m in a.mudancas.iter().chain(b.mudancas.iter()) {
        assert!(
            !m.contains("9999") && !m.contains("7777"),
            "mudanca com numero: {m}"
        );
    }
}

#[test]
fn remover_tira_de_todas_as_listas_e_e_idempotente() {
    let mut s = secao(json!({
        "allow": [USUARIO, "+55 11 99999-8888"],
        "owners": [DONO],
        "access": { "users": { USUARIO: { "level": "read" }, OUTRO: { "blocked": true } } }
    }));
    let a = mutacao::aplicar(&mut s, &Mutacao::Remover(USUARIO.to_string())).expect("aplica");
    assert!(a.mudou);
    assert_eq!(principal(&s, USUARIO), Principal::Estranho);
    assert!(
        s.settings["allow"].as_array().expect("lista").is_empty(),
        "as duas grafias sairam"
    );
    assert!(s.settings["access"]["users"].get(USUARIO).is_none());
    assert_eq!(principal(&s, DONO), Principal::Dono, "os outros ficam");
    assert_eq!(
        principal(&s, OUTRO),
        Principal::Bloqueado,
        "bloqueio de outro fica"
    );
    // Remover o dono legado.
    let b = mutacao::aplicar(&mut s, &Mutacao::Remover(DONO.to_string())).expect("aplica");
    assert!(b.mudou);
    assert_eq!(principal(&s, DONO), Principal::Estranho);
    // Remover quem esta bloqueado remove o bloqueio tambem (a entrada inteira).
    mutacao::aplicar(&mut s, &Mutacao::Remover(OUTRO.to_string())).expect("aplica");
    assert_eq!(principal(&s, OUTRO), Principal::Estranho);
    // Idempotente: quem nao esta em lugar nenhum.
    let c = mutacao::aplicar(&mut s, &Mutacao::Remover(USUARIO.to_string())).expect("aplica");
    assert!(!c.mudou);
    assert!(c.mudancas.is_empty());
    assert_eq!(Mutacao::Remover(USUARIO.to_string()).acao(), "remove");
    assert_eq!(
        Mutacao::Papel {
            identidade: USUARIO.to_string(),
            dono: true
        }
        .acao(),
        "owner"
    );
    assert_eq!(
        Mutacao::Papel {
            identidade: USUARIO.to_string(),
            dono: false
        }
        .acao(),
        "unowner"
    );
}
