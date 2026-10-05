//! Exportar/importar a politica sem segredo (#1435).
//!
//! As provas que importam: o export NUNCA carrega segredo (varredura por
//! valores-sentinela plantados na secao), a versao e validada (formato
//! desconhecido, versao de major futuro), o export redigido nao se reimporta,
//! e o import substitui a politica pelo mesmo motor do turno — com o
//! alargamento classificado pela mesma `impacto::diferencas` do `--dry-run`.

use std::collections::HashMap;

use garraia_agents::modes::Nivel;
use garraia_config::{ChannelConfig, ExecutionProfile};
use serde_json::{Value, json};

use crate::bootstrap::whatsapp_linked::politica::transferencia::{
    self, ErroDeImportacao, FORMATO, VERSAO_ATUAL,
};
use crate::bootstrap::whatsapp_linked::politica::{Admission, Alcance, impacto};
use crate::bootstrap::whatsapp_linked::{LinkedSettings, settings_from_config};

const DONO: &str = "5511977776666";
const USUARIO: &str = "5511999998888";
const GRUPO: &str = "120363000000000000@g.us";

fn secao(settings: Value) -> ChannelConfig {
    let mapa: HashMap<String, Value> = settings
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

/// Uma politica rica: dono, usuario com nivel, bloqueado, grupos.
fn politica_rica() -> ChannelConfig {
    secao(json!({
        "owners": [DONO],
        "access": {
            "admission": "open",
            "default": { "level": "chat", "write": false },
            "users": {
                USUARIO: { "level": "read", "write": true },
                "5521955554444": { "blocked": true },
            },
            "groups": {
                "enabled": true,
                "default": { "level": "read", "write": false },
                GRUPO: { "level": "full", "write": false },
            },
        },
    }))
}

// ---------------------------------------------------------------------------
// Exclusao de segredo
// ---------------------------------------------------------------------------

#[test]
fn export_nunca_carrega_segredo_plantado() {
    // Valores-sentinela plantados na secao (chaves que o export NAO le). Se
    // qualquer um aparecer na saida, a allowlist vazou.
    const SENTINELAS: [&str; 4] = [
        "SENTINELA_TOKEN_abc123",
        "SENTINELA_APIKEY_def456",
        "SENTINELA_SESSION_ghi789",
        "SENTINELA_VAULT_jkl012",
    ];
    let s = secao(json!({
        "owners": [DONO],
        "allow": [USUARIO],
        // Segredos que alguem poderia ter plantado na mesma secao — o export
        // le so `access`, entao nada disto pode sair.
        "access_token": SENTINELAS[0],
        "api_key": SENTINELAS[1],
        "session": SENTINELAS[2],
        "vault_passphrase": SENTINELAS[3],
        "access": {
            "admission": "restricted",
            "default": { "level": "chat", "write": false },
            "users": { USUARIO: { "level": "read", "write": false } },
        },
    }));
    let doc = transferencia::exportar(&settings_de(&s), false);
    let texto = serde_json::to_string(&doc).unwrap();
    for sentinela in SENTINELAS {
        assert!(
            !texto.contains(sentinela),
            "o export vazou o segredo plantado `{sentinela}`:\n{texto}"
        );
    }
    // E a estrutura da politica esta la (nao e um export vazio por acidente).
    assert_eq!(doc["format"], json!(FORMATO));
    assert_eq!(doc["version"], json!(VERSAO_ATUAL));
    assert_eq!(doc["policy"]["admission"], json!("restricted"));
}

#[test]
fn export_marca_e_redige_pii_so_quando_pedido() {
    // Um fixo (nao e celular BR, entao a chave do portao nao mexe nele):
    // a identidade inteira e previsivel na saida.
    const FIXO: &str = "551133334444";
    let s = secao(json!({
        "access": {
            "admission": "restricted",
            "users": { FIXO: { "level": "read", "write": false } },
        },
    }));

    let claro = transferencia::exportar(&settings_de(&s), false);
    let texto_claro = serde_json::to_string(&claro).unwrap();
    assert_eq!(claro["pii_redacted"], json!(false));
    assert!(
        texto_claro.contains(FIXO),
        "o export em claro tem de trazer a identidade inteira: {texto_claro}"
    );

    let redigido = transferencia::exportar(&settings_de(&s), true);
    let texto_redigido = serde_json::to_string(&redigido).unwrap();
    assert_eq!(redigido["pii_redacted"], json!(true));
    assert!(
        !texto_redigido.contains(FIXO),
        "o export redigido NAO pode trazer a identidade inteira"
    );
    assert!(
        texto_redigido.contains("…4444"),
        "o export redigido traz os quatro ultimos digitos: {texto_redigido}"
    );
}

// ---------------------------------------------------------------------------
// Validacao de versao e formato
// ---------------------------------------------------------------------------

#[test]
fn import_recusa_formato_desconhecido() {
    let doc = json!({ "format": "outra-coisa", "version": 1, "policy": {} });
    let erro = transferencia::parse_importacao(&doc).unwrap_err();
    assert!(matches!(erro, ErroDeImportacao::FormatoDesconhecido(_)));
    assert_eq!(erro.codigo(), "import_format_unknown");
}

#[test]
fn import_recusa_versao_de_major_futuro() {
    let doc = json!({ "format": FORMATO, "version": VERSAO_ATUAL + 1, "policy": {} });
    let erro = transferencia::parse_importacao(&doc).unwrap_err();
    assert!(matches!(erro, ErroDeImportacao::VersaoFutura(_)));
    assert_eq!(erro.codigo(), "import_version_future");
}

#[test]
fn import_recusa_versao_ausente_ou_zero() {
    for v in [json!(0), Value::Null] {
        let doc = json!({ "format": FORMATO, "version": v, "policy": {} });
        let erro = transferencia::parse_importacao(&doc).unwrap_err();
        assert!(
            matches!(erro, ErroDeImportacao::VersaoInvalida(_)),
            "versao {v:?} devia ser invalida"
        );
    }
}

#[test]
fn import_aceita_a_versao_atual() {
    let doc = json!({
        "format": FORMATO, "version": VERSAO_ATUAL,
        "policy": { "admission": "restricted" },
    });
    assert!(transferencia::parse_importacao(&doc).is_ok());
}

#[test]
fn import_recusa_export_redigido() {
    let s = politica_rica();
    let redigido = transferencia::exportar(&settings_de(&s), true);
    let erro = transferencia::parse_importacao(&redigido).unwrap_err();
    assert!(matches!(erro, ErroDeImportacao::PiiRedigido));
    assert_eq!(erro.codigo(), "import_pii_redacted");
}

#[test]
fn import_recusa_nivel_desconhecido() {
    let doc = json!({
        "format": FORMATO, "version": 1,
        "policy": { "users": [ { "identity": USUARIO, "level": "wizard" } ] },
    });
    let erro = transferencia::parse_importacao(&doc).unwrap_err();
    assert!(matches!(erro, ErroDeImportacao::Valor(_)));
}

#[test]
fn import_recusa_canal_diferente() {
    let doc = json!({
        "format": FORMATO, "version": 1, "channel": "telegram",
        "policy": {},
    });
    let erro = transferencia::parse_importacao(&doc).unwrap_err();
    assert!(matches!(erro, ErroDeImportacao::CanalDiferente(_)));
}

// ---------------------------------------------------------------------------
// Round-trip e aplicacao
// ---------------------------------------------------------------------------

#[test]
fn export_depois_import_reproduz_a_politica() {
    let origem = politica_rica();
    let doc = transferencia::exportar(&settings_de(&origem), false);
    let importada = transferencia::parse_importacao(&doc).unwrap();

    // Aplica num destino VAZIO (como outra instalacao).
    let mut destino = secao(json!({}));
    transferencia::aplicar_importada(&mut destino, &importada).unwrap();

    let s_origem = settings_de(&origem);
    let s_destino = settings_de(&destino);
    // A politica efetiva e identica, principal a principal, perfil a perfil.
    assert_eq!(s_origem.access.admission, s_destino.access.admission);
    assert_eq!(s_origem.access.default, s_destino.access.default);
    assert_eq!(s_origem.responde_em_grupo(), s_destino.responde_em_grupo());
    assert_eq!(
        s_origem.access.groups.default,
        s_destino.access.groups.default
    );
    assert_eq!(
        s_origem.access.groups.por_grupo,
        s_destino.access.groups.por_grupo
    );
    // Dono, usuario com nivel e bloqueado preservados.
    assert!(s_destino.e_dono(DONO));
    let u = s_destino.entrada_de(USUARIO).unwrap();
    assert_eq!(
        u.alcance,
        Alcance {
            nivel: Nivel::Read,
            write: true
        }
    );
    assert!(s_destino.entrada_de("5521955554444").unwrap().bloqueado);
}

#[test]
fn import_substitui_a_politica_e_limpa_o_legado() {
    // Destino com legado proprio: `allow`/`owners` que o import tem de apagar.
    let mut destino = secao(json!({
        "allow": ["5511911111111"],
        "owners": ["5511922222222"],
        "reply_in_groups": true,
        "default_mode": "code",
    }));
    let origem = politica_rica();
    let doc = transferencia::exportar(&settings_de(&origem), false);
    let importada = transferencia::parse_importacao(&doc).unwrap();
    transferencia::aplicar_importada(&mut destino, &importada).unwrap();

    // O legado some do arquivo: a politica agora e so o import.
    assert!(!destino.settings.contains_key("allow"));
    assert!(!destino.settings.contains_key("owners"));
    assert!(!destino.settings.contains_key("reply_in_groups"));
    // O que NAO e politica fica (o import replica politica, nao o resto).
    assert_eq!(destino.settings.get("default_mode"), Some(&json!("code")));

    let s = settings_de(&destino);
    // Os numeros do legado antigo nao entram mais.
    assert!(s.entrada_de("5511911111111").is_none());
    assert!(s.entrada_de("5511922222222").is_none());
    // Os do import, sim.
    assert!(s.e_dono(DONO));
}

#[test]
fn alargamento_e_detectado_pelo_mesmo_motor() {
    // Destino restrito; import abre a admissao e da nivel a um usuario novo.
    let mut destino = secao(json!({
        "access": { "admission": "restricted", "default": { "level": "chat", "write": false } },
    }));
    let origem = politica_rica(); // admission open, usuario read+write, grupos on
    let doc = transferencia::exportar(&settings_de(&origem), false);
    let importada = transferencia::parse_importacao(&doc).unwrap();

    let antes = settings_de(&destino);
    transferencia::aplicar_importada(&mut destino, &importada).unwrap();
    let depois = settings_de(&destino);

    let difs = impacto::diferencas(&antes, &depois, ExecutionProfile::Standard);
    assert!(
        transferencia::ha_alargamento(&difs),
        "abrir a admissao e dar leitura a um usuario tem de contar como alargamento"
    );
}

#[test]
fn estreitar_nao_e_alargamento() {
    // Origem permissiva (admissao aberta, grupos, usuario com read+write) →
    // destino seguro (tudo fechado). Isso so ESTREITA: sem alargamento.
    let mut destino = politica_rica();
    let origem = secao(json!({
        "access": {
            "admission": "restricted",
            "default": { "level": "chat", "write": false },
            "groups": { "enabled": false },
        },
    }));
    let doc = transferencia::exportar(&settings_de(&origem), false);
    let importada = transferencia::parse_importacao(&doc).unwrap();

    let antes = settings_de(&destino);
    transferencia::aplicar_importada(&mut destino, &importada).unwrap();
    let depois = settings_de(&destino);

    let difs = impacto::diferencas(&antes, &depois, ExecutionProfile::Standard);
    assert!(
        !transferencia::ha_alargamento(&difs),
        "fechar tudo nunca pode ser alargamento:\n{difs:#?}"
    );
}

#[test]
fn aplicar_recusa_secao_de_outro_canal() {
    let mut outra = ChannelConfig {
        channel_type: "telegram".to_string(),
        enabled: Some(true),
        settings: HashMap::new(),
    };
    let importada = transferencia::parse_importacao(&json!({
        "format": FORMATO, "version": 1, "policy": { "admission": "open" },
    }))
    .unwrap();
    assert!(transferencia::aplicar_importada(&mut outra, &importada).is_err());
}

#[test]
fn import_default_full_do_desconhecido_cai_para_read_na_leitura() {
    // Mesmo que um documento traga `default: full` (que nunca devia existir),
    // o motor de leitura (`da_secao`) o normaliza para `read` — fail-closed,
    // como a #1390. O import nao cria um caminho novo para burlar isso.
    let mut destino = secao(json!({}));
    let importada = transferencia::parse_importacao(&json!({
        "format": FORMATO, "version": 1,
        "policy": { "admission": "open", "default": { "level": "full", "write": true } },
    }))
    .unwrap();
    transferencia::aplicar_importada(&mut destino, &importada).unwrap();
    let s = settings_de(&destino);
    assert_eq!(
        s.access.default.nivel,
        Nivel::Read,
        "default do desconhecido nunca e full"
    );
    assert_eq!(s.access.admission, Admission::Open);
}
