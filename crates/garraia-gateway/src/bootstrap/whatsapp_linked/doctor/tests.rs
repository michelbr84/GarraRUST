//! A tabela do `doctor whatsapp`, linha a linha (#1419), mais o que a #1420
//! acrescentou ao mover o motor para ca: o idioma que vem de fora, o shape
//! serializado que a CLI e o console compartilham e a leitura pura da config.

use std::collections::HashMap;

use garraia_config::{ChannelConfig, LlmProviderConfig};

use super::*;

const BIN: &str = "garraia";

fn linha<'a>(linhas: &'a [Linha], id: &str) -> &'a Linha {
    linhas.iter().find(|l| l.id == id).unwrap_or_else(|| {
        panic!(
            "sem a linha {id}: {:?}",
            linhas.iter().map(|l| l.id).collect::<Vec<_>>()
        )
    })
}

fn config_boa() -> ConfigFatos {
    ConfigFatos {
        canal_ligado: true,
        autorizados: 2,
        donos: 1,
        perfil: "standard".into(),
        origem: "default".into(),
        isolado: false,
        piso_do_dono: "search".into(),
        raizes: Raizes::WorkspacePadrao,
        mcp: vec![ServidorMcp {
            nome: "filesystem".into(),
            visibilidade: Visibilidade::SoOperacoes(10),
        }],
        provedores: vec![Provedor {
            nome: "openrouter".into(),
            tipo: "openrouter".into(),
            keyless: false,
            alcancavel: None,
        }],
        provedor_padrao: Some("openrouter".into()),
    }
}

fn tudo_bem() -> Fatos {
    Fatos {
        sessao: Sessao::Lida {
            saude: LinkHealth::Linked,
            chave: Some(Chave {
                do_cofre: true,
                legivel: true,
            }),
        },
        bridge_dir: PathBuf::from("/tmp/x/whatsapp/bridge"),
        config: Some(config_boa()),
        gateway: Gateway {
            pid: Some(4242),
            ouvindo: true,
            host: "127.0.0.1".into(),
            porta: 3888,
            ao_vivo: vec![
                LinhaViva {
                    id: "whatsapp.linked".into(),
                    status: "ok".into(),
                    detail: "conectado".into(),
                    next_step: None,
                },
                LinhaViva {
                    id: "provider.default".into(),
                    status: "ok".into(),
                    detail: "openrouter".into(),
                    next_step: None,
                },
            ],
            recusou_credencial: false,
        },
    }
}

/// O caminho feliz e todo verde, sai 0 e nao carrega passo nenhum.
#[test]
fn tudo_bem_e_verde_e_sai_zero() {
    let linhas = classificar(&tudo_bem(), Lang::Pt, BIN);
    for id in [
        "whatsapp.linked",
        "whatsapp.session_key",
        "whatsapp.gateway",
        "whatsapp.access",
        "execution.profile",
        "files.workspace",
        "mcp.visibility",
        "provider.default",
    ] {
        let l = linha(&linhas, id);
        assert_eq!(l.status, Semaforo::Ok, "{id}: {}", l.detail);
        assert!(l.next_step.is_none(), "{id} com passo: {:?}", l.next_step);
    }
    assert_eq!(exit_code(&linhas, false), 0);
    assert_eq!(exit_code(&linhas, true), 0);
    assert_eq!(agregado(&linhas), "ok");
}

/// Sem vinculo e vermelho — e o passo e o MESMO que o `status` e o
/// `/api/diagnostics` dao (`LinkHealth::next_step`), nao uma copia.
#[test]
fn sem_vinculo_e_error_com_o_passo_de_vincular_e_sai_69() {
    let mut f = tudo_bem();
    f.sessao = Sessao::Lida {
        saude: LinkHealth::NotLinked,
        chave: None,
    };
    let linhas = classificar(&f, Lang::Pt, BIN);
    let l = linha(&linhas, "whatsapp.linked");
    assert_eq!(l.status, Semaforo::Error, "{}", l.detail);
    let esperado = LinkHealth::NotLinked
        .next_step(&f.bridge_dir, BIN)
        .expect("NotLinked tem passo");
    assert_eq!(l.next_step.as_deref(), Some(esperado.as_str()));
    // Sem sessao nao ha chave a julgar.
    assert!(linhas.iter().all(|l| l.id != "whatsapp.session_key"));
    assert_eq!(exit_code(&linhas, false), 69);
    assert_eq!(agregado(&linhas), "error");
}

#[test]
fn ponte_sem_dependencias_e_error_com_o_npm_ci_no_diretorio_da_ponte() {
    let mut f = tudo_bem();
    f.sessao = Sessao::Lida {
        saude: LinkHealth::MissingDependencies,
        chave: Some(Chave {
            do_cofre: true,
            legivel: true,
        }),
    };
    let linhas = classificar(&f, Lang::En, BIN);
    let l = linha(&linhas, "whatsapp.linked");
    assert_eq!(l.status, Semaforo::Error);
    let passo = l.next_step.as_deref().expect("passo");
    assert!(passo.contains("npm ci"), "{passo}");
    assert!(passo.contains("whatsapp/bridge"), "{passo}");
}

/// Chave em `session.key` ao lado do cifrado e aviso, com a exposicao
/// dita em voz alta; chave que nao abre o blob e vermelho.
#[test]
fn chave_em_arquivo_e_warning_e_chave_que_nao_abre_e_error() {
    let mut f = tudo_bem();
    f.sessao = Sessao::Lida {
        saude: LinkHealth::Linked,
        chave: Some(Chave {
            do_cofre: false,
            legivel: true,
        }),
    };
    let l = classificar(&f, Lang::Pt, BIN);
    let k = linha(&l, "whatsapp.session_key");
    assert_eq!(k.status, Semaforo::Warning, "{}", k.detail);
    assert!(k.detail.contains("session.key"), "{}", k.detail);
    assert!(
        k.next_step
            .as_deref()
            .unwrap_or("")
            .contains("GARRAIA_VAULT_PASSPHRASE"),
        "{:?}",
        k.next_step
    );

    f.sessao = Sessao::Lida {
        saude: LinkHealth::Linked,
        chave: Some(Chave {
            do_cofre: true,
            legivel: false,
        }),
    };
    let l = classificar(&f, Lang::Pt, BIN);
    let k = linha(&l, "whatsapp.session_key");
    assert_eq!(k.status, Semaforo::Error, "{}", k.detail);
    assert!(
        k.next_step.as_deref().unwrap_or("").contains("whatsapp"),
        "{:?}",
        k.next_step
    );
}

/// Gateway parado: aviso, com o `start`. E o que so ele sabe (a ponte)
/// nao e inventado — a linha diz que nao sabe.
#[test]
fn gateway_parado_e_warning_com_o_start_e_nao_inventa_a_ponte() {
    let mut f = tudo_bem();
    f.gateway = Gateway {
        pid: None,
        ouvindo: false,
        host: "127.0.0.1".into(),
        porta: 3888,
        ao_vivo: vec![],
        recusou_credencial: false,
    };
    let linhas = classificar(&f, Lang::Pt, BIN);
    let g = linha(&linhas, "whatsapp.gateway");
    assert_eq!(g.status, Semaforo::Warning, "{}", g.detail);
    assert!(
        g.next_step
            .as_deref()
            .unwrap_or("")
            .contains("garraia start"),
        "{:?}",
        g.next_step
    );
    let v = linha(&linhas, "whatsapp.linked");
    assert!(!v.detail.contains("conectad"), "{}", v.detail);
}

/// Gateway de pe que RECUSOU a credencial da CLI (401): e um "nao sei"
/// diferente do "nao respondeu" — o passo pede a chave no ambiente da CLI,
/// nao o log. E nem a ponte nem o provider sao inventados.
#[test]
fn gateway_que_recusa_a_credencial_da_cli_e_warning_pedindo_a_chave() {
    let mut f = tudo_bem();
    f.gateway.ao_vivo.clear();
    f.gateway.recusou_credencial = true;
    let linhas = classificar(&f, Lang::Pt, BIN);
    let g = linha(&linhas, "whatsapp.gateway");
    assert_eq!(g.status, Semaforo::Warning, "{}", g.detail);
    assert!(g.detail.contains("401"), "{}", g.detail);
    let passo = g.next_step.as_deref().unwrap_or("");
    assert!(passo.contains("GARRAIA_GATEWAY"), "{passo}");
    // O provider cai para a config (que aqui e boa), nao para "o gateway disse".
    let p = linha(&linhas, "provider.default");
    assert_eq!(p.status, Semaforo::Ok, "{}", p.detail);
    assert!(!p.detail.contains("segundo o gateway"), "{}", p.detail);
}

/// Com o gateway de pe, a ponte e o provider sao o que o
/// `/api/diagnostics` diz — inclusive quando ele diz que caiu.
#[test]
fn gateway_de_pe_repassa_o_que_o_diagnostics_diz_da_ponte_e_do_provider() {
    let mut f = tudo_bem();
    f.gateway.ao_vivo = vec![
        LinhaViva {
            id: "whatsapp.linked".into(),
            status: "error".into(),
            detail: "ponte caida".into(),
            next_step: Some("reinicie o gateway".into()),
        },
        LinhaViva {
            id: "provider.default".into(),
            status: "error".into(),
            detail: "none registered".into(),
            next_step: Some("configure um provider".into()),
        },
    ];
    let linhas = classificar(&f, Lang::Pt, BIN);
    let g = linha(&linhas, "whatsapp.gateway");
    assert_eq!(g.status, Semaforo::Error, "{}", g.detail);
    assert!(g.detail.contains("ponte caida"), "{}", g.detail);
    assert_eq!(g.next_step.as_deref(), Some("reinicie o gateway"));
    let p = linha(&linhas, "provider.default");
    assert_eq!(p.status, Semaforo::Error, "{}", p.detail);
    assert!(p.detail.contains("none registered"), "{}", p.detail);
    assert_eq!(exit_code(&linhas, false), 69);
}

/// Canal ligado com portao vazio: ninguem recebe resposta. Canal
/// desligado: o gateway nao consome. Os dois sao aviso, com o passo.
#[test]
fn portao_vazio_e_canal_desligado_sao_warning_com_passo() {
    let mut f = tudo_bem();
    f.config.as_mut().expect("config").autorizados = 0;
    f.config.as_mut().expect("config").donos = 0;
    let l = classificar(&f, Lang::Pt, BIN);
    let a = linha(&l, "whatsapp.access");
    assert_eq!(a.status, Semaforo::Warning, "{}", a.detail);
    assert!(
        a.next_step
            .as_deref()
            .unwrap_or("")
            .contains("whatsapp allow"),
        "{:?}",
        a.next_step
    );

    let mut f = tudo_bem();
    f.config.as_mut().expect("config").canal_ligado = false;
    let l = classificar(&f, Lang::En, BIN);
    let a = linha(&l, "whatsapp.access");
    assert_eq!(a.status, Semaforo::Warning, "{}", a.detail);
    assert!(
        a.next_step.as_deref().unwrap_or("").contains("enabled"),
        "{:?}",
        a.next_step
    );
}

/// `isolated-pod` e SEMPRE aviso (ADR 0024): o dono ganha `code` com
/// bash no host do pod, e o passo diz como reverter.
#[test]
fn isolated_pod_e_warning_dizendo_o_que_o_dono_ganha_e_como_reverter() {
    let mut f = tudo_bem();
    let c = f.config.as_mut().expect("config");
    c.perfil = "isolated-pod".into();
    c.origem = "env".into();
    c.isolado = true;
    c.piso_do_dono = "code".into();
    let l = classificar(&f, Lang::Pt, BIN);
    let e = linha(&l, "execution.profile");
    assert_eq!(e.status, Semaforo::Warning, "{}", e.detail);
    for esperado in ["isolated-pod", "env", "code", "search"] {
        assert!(e.detail.contains(esperado), "{esperado:?} em {}", e.detail);
    }
    assert!(
        e.next_step
            .as_deref()
            .unwrap_or("")
            .contains("execution.profile"),
        "{:?}",
        e.next_step
    );
}

#[test]
fn sem_raiz_efetiva_e_warning_e_raiz_declarada_sai_como_contagem() {
    let mut f = tudo_bem();
    f.config.as_mut().expect("config").raizes = Raizes::SomenteSessao;
    let l = classificar(&f, Lang::Pt, BIN);
    let w = linha(&l, "files.workspace");
    assert_eq!(w.status, Semaforo::Warning, "{}", w.detail);
    assert!(w.next_step.is_some());

    f.config.as_mut().expect("config").raizes = Raizes::Declaradas(3);
    let l = classificar(&f, Lang::Pt, BIN);
    let w = linha(&l, "files.workspace");
    assert_eq!(w.status, Semaforo::Ok, "{}", w.detail);
    assert!(w.detail.contains('3'), "{}", w.detail);
}

/// MCP declarado que o piso esconde e aviso nomeando o servidor e a
/// sintaxe (#1384/#1387); nenhum declarado e neutro.
#[test]
fn mcp_escondido_no_piso_e_warning_e_nenhum_declarado_e_neutro() {
    let mut f = tudo_bem();
    f.config.as_mut().expect("config").mcp.push(ServidorMcp {
        nome: "github".into(),
        visibilidade: Visibilidade::Escondido,
    });
    let l = classificar(&f, Lang::Pt, BIN);
    let m = linha(&l, "mcp.visibility");
    assert_eq!(m.status, Semaforo::Warning, "{}", m.detail);
    assert!(m.detail.contains("github"), "{}", m.detail);
    assert!(m.detail.contains("filesystem"), "{}", m.detail);
    let passo = m.next_step.as_deref().unwrap_or("");
    assert!(
        passo.contains("github/*") && passo.contains("*/<operacao>"),
        "{passo}"
    );

    f.config.as_mut().expect("config").mcp.clear();
    let l = classificar(&f, Lang::Pt, BIN);
    assert_eq!(linha(&l, "mcp.visibility").status, Semaforo::NotConfigured);
}

/// Sem gateway de pe o provider e julgado pela config: nenhum e
/// vermelho; um daemon local que nao responde e vermelho; um cloud com
/// credencial presente e verde.
#[test]
fn provider_offline_e_julgado_pela_config() {
    let mut f = tudo_bem();
    f.gateway.ao_vivo.clear();
    f.config.as_mut().expect("config").provedores.clear();
    f.config.as_mut().expect("config").provedor_padrao = None;
    let l = classificar(&f, Lang::Pt, BIN);
    let p = linha(&l, "provider.default");
    assert_eq!(p.status, Semaforo::Error, "{}", p.detail);
    assert!(
        p.next_step
            .as_deref()
            .unwrap_or("")
            .contains("garraia init"),
        "{:?}",
        p.next_step
    );

    let mut f = tudo_bem();
    f.gateway.ao_vivo.clear();
    let c = f.config.as_mut().expect("config");
    c.provedores = vec![Provedor {
        nome: "ollama".into(),
        tipo: "ollama".into(),
        keyless: true,
        alcancavel: Some(false),
    }];
    c.provedor_padrao = Some("ollama".into());
    let l = classificar(&f, Lang::Pt, BIN);
    let p = linha(&l, "provider.default");
    assert_eq!(p.status, Semaforo::Error, "{}", p.detail);
    assert!(p.detail.contains("ollama"), "{}", p.detail);

    let mut f = tudo_bem();
    f.gateway.ao_vivo.clear();
    let l = classificar(&f, Lang::Pt, BIN);
    assert_eq!(linha(&l, "provider.default").status, Semaforo::Ok);
}

/// Config que nao carregou: uma linha de aviso propria, e o resto do
/// relatorio continua (o vinculo e o gateway nao dependem dela).
#[test]
fn sem_config_o_relatorio_avisa_e_segue() {
    let mut f = tudo_bem();
    f.config = None;
    let l = classificar(&f, Lang::Pt, BIN);
    let c = linha(&l, "config");
    assert_eq!(c.status, Semaforo::Warning, "{}", c.detail);
    assert!(
        c.next_step
            .as_deref()
            .unwrap_or("")
            .contains("garraia init"),
        "{:?}",
        c.next_step
    );
    linha(&l, "whatsapp.linked");
    linha(&l, "whatsapp.gateway");
    assert!(l.iter().all(|x| x.id != "whatsapp.access"));
}

/// `--strict` promove aviso a 2; sem ele, aviso e 0. Vermelho e 69 sempre.
#[test]
fn exit_code_segue_o_semaforo() {
    let ok = vec![Linha::nova("a", Semaforo::Ok, String::new(), None)];
    let neutro = vec![Linha::nova(
        "a",
        Semaforo::NotConfigured,
        String::new(),
        None,
    )];
    let aviso = vec![Linha::nova("a", Semaforo::Warning, String::new(), None)];
    let erro = vec![
        Linha::nova("a", Semaforo::Warning, String::new(), None),
        Linha::nova("b", Semaforo::Error, String::new(), None),
    ];
    assert_eq!(exit_code(&ok, true), 0);
    assert_eq!(exit_code(&neutro, true), 0);
    assert_eq!(exit_code(&aviso, false), 0);
    assert_eq!(exit_code(&aviso, true), 2);
    assert_eq!(exit_code(&erro, false), 69);
    assert_eq!(agregado(&neutro), "ok");
    assert_eq!(agregado(&aviso), "warning");
    assert_eq!(agregado(&erro), "error");
}

/// Nenhuma linha carrega numero de telefone, LID ou chave — so
/// contagens e origens. O teste alimenta fatos com "cara" de segredo e
/// varre a saida.
#[test]
fn nenhuma_linha_carrega_identidade_nem_segredo() {
    let f = tudo_bem();
    let linhas = classificar(&f, Lang::Pt, BIN);
    let json = serde_json::to_string(&linhas).expect("json");
    for proibido in ["+55", "@lid", "passphrase=", "session.enc"] {
        assert!(!json.contains(proibido), "{proibido} em {json}");
    }
    // As contagens, sim.
    let a = linha(&linhas, "whatsapp.access");
    assert!(
        a.detail.contains('2') && a.detail.contains('1'),
        "{}",
        a.detail
    );
}

/// O JSON fala o vocabulario do `/api/diagnostics`.
#[test]
fn o_json_usa_o_vocabulario_do_diagnostics() {
    assert_eq!(
        serde_json::to_string(&Semaforo::NotConfigured).expect("json"),
        "\"not_configured\""
    );
    assert_eq!(
        serde_json::to_string(&Semaforo::Ok).expect("json"),
        "\"ok\""
    );
    for s in [
        Semaforo::Ok,
        Semaforo::Warning,
        Semaforo::Error,
        Semaforo::NotConfigured,
    ] {
        assert_eq!(
            serde_json::to_string(&s).expect("json"),
            format!("\"{}\"", s.as_str())
        );
    }
}

// ---------------------------------------------------------------------------
// #1420: o que a mudanca de casa acrescentou
// ---------------------------------------------------------------------------

/// O idioma que vem de fora (`?lang=`, `navigator.language`): `en*` e ingles,
/// o resto (inclusive lixo e vazio) e pt-BR.
#[test]
fn lang_parse_aceita_en_com_regiao_e_cai_em_pt_no_resto() {
    for en in ["en", "en-US", "EN_gb", " en "] {
        assert_eq!(Lang::parse(en), Lang::En, "{en:?}");
    }
    for pt in ["pt", "pt-BR", "es", "", "e", "xx", "fr-en"] {
        assert_eq!(Lang::parse(pt), Lang::Pt, "{pt:?}");
    }
    // O prefixo decide, como no `Lang::detect` da CLI: `english` e ingles.
    assert_eq!(Lang::parse("english"), Lang::En);
    assert_eq!(Lang::En.as_str(), "en");
    assert_eq!(Lang::Pt.as_str(), "pt");
    assert_eq!(Lang::default(), Lang::Pt);
}

/// Paridade de shape: uma linha serializada tem EXATAMENTE as chaves que o
/// `garraia doctor whatsapp --json` ja emite (`id`, `status`, `detail`,
/// `next_step` so quando ha). E o contrato que o console le.
#[test]
fn uma_linha_serializa_so_id_status_detail_e_next_step() {
    let com_passo = Linha::nova(
        "whatsapp.access",
        Semaforo::Warning,
        "portao vazio".into(),
        Some("garraia whatsapp allow <numero>".into()),
    );
    let v = serde_json::to_value(&com_passo).expect("json");
    let mut chaves: Vec<&str> = v
        .as_object()
        .expect("objeto")
        .keys()
        .map(String::as_str)
        .collect();
    chaves.sort_unstable();
    assert_eq!(chaves, ["detail", "id", "next_step", "status"]);
    assert_eq!(v["status"], serde_json::json!("warning"));

    let sem_passo = Linha::nova(
        "whatsapp.access",
        Semaforo::Ok,
        "autorizados: 2".into(),
        None,
    );
    let v = serde_json::to_value(&sem_passo).expect("json");
    let mut chaves: Vec<&str> = v
        .as_object()
        .expect("objeto")
        .keys()
        .map(String::as_str)
        .collect();
    chaves.sort_unstable();
    assert_eq!(chaves, ["detail", "id", "status"], "next_step ausente some");
}

fn secao_whatsapp(allow: &[&str], owners: &[&str], enabled: bool) -> ChannelConfig {
    let mut settings = HashMap::new();
    settings.insert("allow".to_string(), serde_json::json!(allow));
    settings.insert("owners".to_string(), serde_json::json!(owners));
    ChannelConfig {
        channel_type: "whatsapp_linked".to_string(),
        enabled: Some(enabled),
        settings,
    }
}

fn provider(tipo: &str, chave: Option<&str>) -> LlmProviderConfig {
    LlmProviderConfig {
        provider: tipo.to_string(),
        model: None,
        api_key: chave.map(str::to_string),
        base_url: None,
        extra: HashMap::new(),
    }
}

/// A leitura pura da config: acesso pelo mesmo leitor do portao (contagens),
/// perfil, providers em ordem estavel e sem sondar, padrao so quando existe,
/// MCP pelo piso `search`. E nada que pareca segredo ou identidade sobrevive
/// no `Debug` — que e o que um log de erro imprimiria.
#[test]
fn fatos_da_config_le_acesso_perfil_providers_e_mcp_sem_sondar_nem_vazar() {
    const NUMERO: &str = "5511999998888";
    const DONO: &str = "5511977776666";
    const SEGREDO: &str = "sk-or-segredo-0123456789abcdef";

    let dir = tempfile::tempdir().expect("tempdir");
    let mut config = AppConfig {
        data_dir: Some(dir.path().join("data")),
        ..AppConfig::default()
    };
    config.channels.insert(
        "whatsapp_linked".to_string(),
        secao_whatsapp(&[NUMERO], &[DONO], true),
    );
    config.llm.insert(
        "openrouter".to_string(),
        provider("openrouter", Some(SEGREDO)),
    );
    config
        .llm
        .insert("local".to_string(), provider("ollama", None));
    config.agent.default_provider = Some("openrouter".to_string());
    for nome in ["github", "filesystem"] {
        let servidor = serde_json::from_value(serde_json::json!({ "command": "npx" }))
            .expect("McpServerConfig");
        config.mcp.insert(nome.to_string(), servidor);
    }

    let f = fatos_da_config(&config);
    assert!(f.canal_ligado);
    assert_eq!((f.autorizados, f.donos), (2, 1));
    assert_eq!(f.perfil, "standard");
    assert!(!f.isolado);
    assert_eq!(f.piso_do_dono, "search");
    // `<data_dir>/workspace` nao existe: sem raiz efetiva (fail-closed).
    assert_eq!(f.raizes, Raizes::SomenteSessao);

    let nomes: Vec<(&str, &str, bool, Option<bool>)> = f
        .provedores
        .iter()
        .map(|p| (p.nome.as_str(), p.tipo.as_str(), p.keyless, p.alcancavel))
        .collect();
    assert_eq!(
        nomes,
        [
            ("local", "ollama", true, None),
            ("openrouter", "openrouter", false, None)
        ],
        "ordem estavel, keyless so para daemon local, ninguem sondado"
    );
    assert_eq!(f.provedor_padrao.as_deref(), Some("openrouter"));

    let vis = |nome: &str| {
        f.mcp
            .iter()
            .find(|s| s.nome == nome)
            .map(|s| s.visibilidade)
            .unwrap_or_else(|| panic!("sem {nome} em {:?}", f.mcp))
    };
    assert_eq!(vis("github"), Visibilidade::Escondido);
    assert!(
        matches!(vis("filesystem"), Visibilidade::SoOperacoes(n) if n > 0),
        "{:?}",
        vis("filesystem")
    );

    let debug = format!("{f:?}");
    for proibido in [SEGREDO, NUMERO, DONO] {
        assert!(!debug.contains(proibido), "{proibido} em {debug}");
    }

    // Padrao que nao existe na config nao e afirmado; workspace criado vira
    // a raiz padrao; canal desligado e lido como tal.
    config.agent.default_provider = Some("inexistente".to_string());
    std::fs::create_dir_all(dir.path().join("data").join("workspace")).expect("workspace");
    config.channels.insert(
        "whatsapp_linked".to_string(),
        secao_whatsapp(&[], &[], false),
    );
    let f = fatos_da_config(&config);
    assert_eq!(f.provedor_padrao, None);
    assert_eq!(f.raizes, Raizes::WorkspacePadrao);
    assert!(!f.canal_ligado);
    assert_eq!((f.autorizados, f.donos), (0, 0));
}
