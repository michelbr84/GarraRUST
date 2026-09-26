//! A tabela do `doctor whatsapp`: fatos → linhas. Pura, sem I/O, sem relogio
//! — e por isso testavel linha a linha em `tests.rs`.

use garraia_channels::whatsapp_linked::KeyOrigin;

use super::*;

impl Linha {
    fn nova(id: &'static str, status: Semaforo, detail: String, next_step: Option<String>) -> Self {
        Self {
            id,
            status,
            detail,
            next_step,
        }
    }
}

/// O status de uma linha do gateway vivo, no nosso semaforo. Desconhecido e
/// neutro — o contrato do `/api/diagnostics` e aditivo (#1437).
fn semaforo_do_gateway(status: &str) -> Semaforo {
    match status {
        "ok" => Semaforo::Ok,
        "warning" => Semaforo::Warning,
        "error" => Semaforo::Error,
        _ => Semaforo::NotConfigured,
    }
}

/// A tabela: fatos → linhas. Pura, sem I/O, sem relogio.
pub(crate) fn classificar(f: &Fatos, lang: Lang, bin: &str) -> Vec<Linha> {
    let mut out = Vec::new();
    let vivo = |id: &str| f.gateway.ao_vivo.iter().find(|l| l.id == id);

    // 1. O vinculo, pelo disco — a mesma tabela do `status` e do console.
    match &f.sessao {
        Sessao::Indisponivel(erro) => out.push(Linha::nova(
            "whatsapp.linked",
            Semaforo::Error,
            format!(
                "{} {erro}",
                t(
                    lang,
                    "diretorio da sessao inacessivel:",
                    "session directory unavailable:"
                )
            ),
            None,
        )),
        Sessao::Lida { saude, chave } => {
            let (status, detalhe) = match saude {
                LinkHealth::NotLinked => (
                    Semaforo::Error,
                    t(
                        lang,
                        "nenhum WhatsApp pessoal vinculado",
                        "no personal WhatsApp linked",
                    ),
                ),
                LinkHealth::MissingDependencies => (
                    Semaforo::Error,
                    t(
                        lang,
                        "vinculado, mas a ponte esta sem dependencias instaladas",
                        "linked, but the bridge has no dependencies installed",
                    ),
                ),
                LinkHealth::BridgeDown => (
                    Semaforo::Error,
                    t(
                        lang,
                        "vinculado, e a ponte esta caida",
                        "linked, and the bridge is down",
                    ),
                ),
                LinkHealth::Connected => (
                    Semaforo::Ok,
                    t(
                        lang,
                        "vinculado e ponte conectada",
                        "linked and bridge connected",
                    ),
                ),
                LinkHealth::Linked => (
                    Semaforo::Ok,
                    t(
                        lang,
                        "vinculado (sessao e dependencias em disco; o estado da ponte e o gateway \
                         quem sabe — veja whatsapp.gateway)",
                        "linked (session and dependencies on disk; only the gateway knows the \
                         bridge state — see whatsapp.gateway)",
                    ),
                ),
            };
            let passo = match lang {
                Lang::Pt => saude.next_step(&f.bridge_dir, bin),
                Lang::En => saude.next_step_en(&f.bridge_dir, bin),
            };
            out.push(Linha::nova(
                "whatsapp.linked",
                status,
                detalhe.to_string(),
                passo,
            ));

            // 2. A chave: origem e prova de que abre. O aviso da chave em
            // arquivo e o MESMO do `status` (`KeyOrigin::warning`).
            if let Some(c) = chave {
                let (status, detalhe, passo) = if !c.legivel {
                    (
                        Semaforo::Error,
                        t(
                            lang,
                            "a chave nao abre a sessao gravada (a passphrase do cofre mudou, ou \
                             o arquivo esta corrompido)",
                            "the key does not open the stored session (the vault passphrase \
                             changed, or the file is corrupt)",
                        )
                        .to_string(),
                        Some(format!(
                            "{bin} whatsapp {}",
                            t(lang, "(vincular de novo)", "(link again)")
                        )),
                    )
                } else if c.do_cofre {
                    (
                        Semaforo::Ok,
                        t(
                            lang,
                            "chave derivada da passphrase do cofre; a sessao abre",
                            "key derived from the vault passphrase; the session opens",
                        )
                        .to_string(),
                        None,
                    )
                } else {
                    let aviso = match lang {
                        Lang::Pt => KeyOrigin::RandomKeyFile.warning(),
                        Lang::En => KeyOrigin::RandomKeyFile.warning_en(),
                    }
                    .unwrap_or_default();
                    (
                        Semaforo::Warning,
                        format!(
                            "{} {aviso}",
                            t(lang, "a sessao abre, mas", "the session opens, but")
                        ),
                        Some(
                            t(
                                lang,
                                "defina GARRAIA_VAULT_PASSPHRASE no ambiente do gateway e da CLI \
                                 (docs/whatsapp.md) — a chave deixa de tocar o disco",
                                "set GARRAIA_VAULT_PASSPHRASE in the gateway's and the CLI's \
                                 environment (docs/whatsapp.md) — the key stops touching the disk",
                            )
                            .to_string(),
                        ),
                    )
                };
                out.push(Linha::nova("whatsapp.session_key", status, detalhe, passo));
            }
        }
    }

    // 3. O gateway: so ele sabe se a ponte esta de pe. Com ele respondendo, a
    // linha e o que o `/api/diagnostics` disse; sem ele, a linha diz que nao
    // sabe — nunca "caiu" nem "conectado" por inferencia.
    let (host, porta) = (&f.gateway.host, f.gateway.porta);
    let pid = f
        .gateway
        .pid
        .map(|p| format!(" (pid {p})"))
        .unwrap_or_default();
    let (status, detalhe, passo) = if f.gateway.ouvindo && f.gateway.recusou_credencial {
        // Um "nao sei" diferente do "nao respondeu": o gateway tem chave de
        // API e esta CLI nao a encontrou — o passo e a chave, nao o log.
        (
            Semaforo::Warning,
            format!(
                "{} {host}:{porta}{pid}, {}",
                t(lang, "porta aberta em", "port open at"),
                t(
                    lang,
                    "mas /api/diagnostics recusou a credencial desta CLI (401): o gateway tem \
                     chave de API e ela nao esta no ambiente/config daqui — o estado da ponte \
                     nao e afirmavel",
                    "but /api/diagnostics refused this CLI's credential (401): the gateway has \
                     an API key and it is not in this CLI's environment/config — the bridge \
                     state cannot be asserted"
                )
            ),
            Some(
                t(
                    lang,
                    "exporte a mesma GARRAIA_GATEWAY_API_KEY (ou gateway.api_key no config.yml) \
                     no ambiente desta CLI e rode de novo",
                    "export the same GARRAIA_GATEWAY_API_KEY (or gateway.api_key in config.yml) \
                     in this CLI's environment and run again",
                )
                .to_string(),
            ),
        )
    } else if f.gateway.ouvindo {
        match vivo("whatsapp.linked") {
            Some(v) => (
                semaforo_do_gateway(&v.status),
                format!(
                    "{} {host}:{porta}{pid} · {}: {}",
                    t(lang, "gateway respondendo em", "gateway answering at"),
                    t(lang, "ponte", "bridge"),
                    v.detail
                ),
                v.next_step.clone(),
            ),
            None => (
                Semaforo::Warning,
                format!(
                    "{} {host}:{porta}, {}",
                    t(lang, "porta aberta em", "port open at"),
                    t(
                        lang,
                        "mas /api/diagnostics nao respondeu — o estado da ponte nao e afirmavel",
                        "but /api/diagnostics did not answer — the bridge state cannot be asserted"
                    )
                ),
                Some(format!(
                    "{bin} status · {bin} logs {}",
                    t(lang, "(veja o log do gateway)", "(check the gateway log)")
                )),
            ),
        }
    } else {
        (
            Semaforo::Warning,
            t(
                lang,
                "gateway parado: o canal so recebe e responde com o gateway de pe",
                "gateway not running: the channel only receives and answers while the gateway is up",
            )
            .to_string(),
            Some(format!("{bin} start")),
        )
    };
    out.push(Linha::nova("whatsapp.gateway", status, detalhe, passo));

    // 4–8. O que depende da config.
    let Some(c) = &f.config else {
        out.push(Linha::nova(
            "config",
            Semaforo::Warning,
            t(
                lang,
                "config nao carregou (ausente ou invalida): acesso, perfil, workspace, MCP e \
                 provider nao puderam ser verificados",
                "config did not load (missing or invalid): access, profile, workspace, MCP and \
                 provider could not be checked",
            )
            .to_string(),
            Some(format!(
                "{bin} init {} {bin} config check",
                t(lang, "para criar, ou", "to create it, or")
            )),
        ));
        return out;
    };

    // 4. Acesso: canal ligado, portao povoado. Contagens, nunca identidades.
    let (status, detalhe, passo) = if !c.canal_ligado {
        (
            Semaforo::Warning,
            t(
                lang,
                "canal desligado na config: o gateway nao consome este canal",
                "channel disabled in the config: the gateway does not consume this channel",
            )
            .to_string(),
            Some(
                t(
                    lang,
                    "ligue `channels.whatsapp_linked.enabled: true` no config.yml e reinicie o gateway",
                    "set `channels.whatsapp_linked.enabled: true` in config.yml and restart the gateway",
                )
                .to_string(),
            ),
        )
    } else if c.autorizados == 0 {
        (
            Semaforo::Warning,
            t(
                lang,
                "canal ligado com o portao vazio: ninguem recebe resposta",
                "channel enabled with an empty gate: nobody gets an answer",
            )
            .to_string(),
            Some(format!(
                "{bin} whatsapp allow <{}>",
                t(lang, "numero", "number")
            )),
        )
    } else {
        (
            Semaforo::Ok,
            format!(
                "{} {} · {} {}",
                t(lang, "autorizados:", "authorized:"),
                c.autorizados,
                t(lang, "donos:", "owners:"),
                c.donos
            ),
            None,
        )
    };
    out.push(Linha::nova("whatsapp.access", status, detalhe, passo));

    // 5. Perfil de execucao (ADR 0024): `isolated-pod` e SEMPRE aviso.
    let modos = format!(
        "{}: search · {}: {}",
        t(lang, "remetente comum", "regular sender"),
        t(lang, "dono em conversa 1:1", "owner in a 1:1 chat"),
        c.piso_do_dono
    );
    let (status, detalhe, passo) = if c.isolado {
        (
            Semaforo::Warning,
            format!(
                "{} ({}: {}) — {modos}; {}",
                c.perfil,
                t(lang, "fonte", "source"),
                c.origem,
                t(
                    lang,
                    "o dono ganha bash no host do pod",
                    "the owner gets bash on the pod's host"
                )
            ),
            Some(
                t(
                    lang,
                    "confirme que este processo roda num pod/container isolado; para reverter: \
                     execution.profile = standard (ou remova GARRAIA_EXECUTION_PROFILE)",
                    "confirm this process runs inside an isolated pod/container; to revert: \
                     execution.profile = standard (or unset GARRAIA_EXECUTION_PROFILE)",
                )
                .to_string(),
            ),
        )
    } else {
        (
            Semaforo::Ok,
            format!(
                "{} ({}: {}) — {modos}",
                c.perfil,
                t(lang, "fonte", "source"),
                c.origem
            ),
            None,
        )
    };
    out.push(Linha::nova("execution.profile", status, detalhe, passo));

    // 6. Raizes das file tools — o mesmo resolvedor do boot.
    let (status, detalhe, passo) = match c.raizes {
        Raizes::Declaradas(n) => (
            Semaforo::Ok,
            format!(
                "{n} {}",
                t(
                    lang,
                    "raiz(es) declarada(s) em agent.file_roots / GARRAIA_FILE_ROOTS (sem escopo por sessao)",
                    "declared root(s) in agent.file_roots / GARRAIA_FILE_ROOTS (no per-session scope)"
                )
            ),
            None,
        ),
        Raizes::WorkspacePadrao => (
            Semaforo::Ok,
            t(
                lang,
                "<data_dir>/workspace/<sessao>: cada conversa ganha o seu diretorio",
                "<data_dir>/workspace/<session>: each conversation gets its own directory",
            )
            .to_string(),
            None,
        ),
        Raizes::SomenteSessao => (
            Semaforo::Warning,
            t(
                lang,
                "sem raiz efetiva: file_read, file_write e list_dir negam tudo",
                "no effective root: file_read, file_write and list_dir deny everything",
            )
            .to_string(),
            Some(
                t(
                    lang,
                    "confira se o <data_dir> existe e e gravavel, ou declare agent.file_roots",
                    "check that <data_dir> exists and is writable, or declare agent.file_roots",
                )
                .to_string(),
            ),
        ),
    };
    out.push(Linha::nova("files.workspace", status, detalhe, passo));

    // 7. MCP no piso do remetente comum (`search`): o que o modelo enxerga.
    let (status, detalhe, passo) = if c.mcp.is_empty() {
        (
            Semaforo::NotConfigured,
            t(
                lang,
                "nenhum servidor MCP declarado (mcp.json / `mcp:` do config.yml)",
                "no MCP server declared (mcp.json / `mcp:` in config.yml)",
            )
            .to_string(),
            None,
        )
    } else {
        let mut visiveis = Vec::new();
        let mut escondidos = Vec::new();
        for s in &c.mcp {
            match s.visibilidade {
                Visibilidade::Inteiro => {
                    visiveis.push(format!("{} ({})", s.nome, t(lang, "inteiro", "all tools")))
                }
                Visibilidade::SoOperacoes(n) => visiveis.push(format!(
                    "{} ({n} {})",
                    s.nome,
                    t(lang, "operacoes de leitura", "read operations")
                )),
                Visibilidade::Escondido => escondidos.push(s.nome.clone()),
            }
        }
        let lista = |v: &[String]| {
            if v.is_empty() {
                t(lang, "(nenhum)", "(none)").to_string()
            } else {
                v.join(", ")
            }
        };
        if escondidos.is_empty() {
            (
                Semaforo::Ok,
                format!(
                    "{}: {}",
                    t(lang, "o piso `search` enxerga", "the `search` floor sees"),
                    lista(&visiveis)
                ),
                None,
            )
        } else {
            (
                Semaforo::Warning,
                format!(
                    "{}: {} · {}: {}",
                    t(
                        lang,
                        "visiveis no piso `search`",
                        "visible at the `search` floor"
                    ),
                    lista(&visiveis),
                    t(
                        lang,
                        "escondidos (o modelo nem os ve)",
                        "hidden (the model does not even see them)"
                    ),
                    escondidos.join(", ")
                ),
                Some(format!(
                    "{} `{}/*` {} `*/<operacao>` {} (docs/src/modes.md, #1384)",
                    t(lang, "para liberar, declare", "to expose, declare"),
                    escondidos[0],
                    t(lang, "ou", "or"),
                    t(
                        lang,
                        "na `allowed` de um modo com whitelist",
                        "in the `allowed` of a whitelisted mode"
                    )
                )),
            )
        }
    };
    out.push(Linha::nova("mcp.visibility", status, detalhe, passo));

    // 8. Provider: com o gateway de pe, e o que ele registrou; sem ele, a
    // config e o probe local.
    let (status, detalhe, passo) = if let Some(v) = vivo("provider.default") {
        (
            semaforo_do_gateway(&v.status),
            format!(
                "{} {}",
                t(lang, "segundo o gateway:", "per the gateway:"),
                v.detail
            ),
            v.next_step.clone(),
        )
    } else if c.provedores.is_empty() {
        (
            Semaforo::Error,
            t(
                lang,
                "nenhum provider configurado: o WhatsApp recebe e nao responde",
                "no provider configured: WhatsApp receives and never answers",
            )
            .to_string(),
            Some(format!(
                "{bin} init {} {bin} config set-model",
                t(lang, "ou", "or")
            )),
        )
    } else {
        let padrao = c
            .provedor_padrao
            .clone()
            .or_else(|| (c.provedores.len() == 1).then(|| c.provedores[0].nome.clone()));
        match padrao
            .as_deref()
            .and_then(|n| c.provedores.iter().find(|p| p.nome == n))
        {
            Some(p) if p.alcancavel == Some(false) => (
                Semaforo::Error,
                format!(
                    "{} `{}` ({}) {}",
                    t(lang, "provider padrao", "default provider"),
                    p.nome,
                    p.tipo,
                    t(
                        lang,
                        "nao respondeu ao probe local",
                        "did not answer the local probe"
                    )
                ),
                Some(
                    t(
                        lang,
                        "suba o servidor local (ex.: `ollama serve`) ou troque agent.default_provider",
                        "start the local server (e.g. `ollama serve`) or change agent.default_provider",
                    )
                    .to_string(),
                ),
            ),
            Some(p) => (
                Semaforo::Ok,
                format!(
                    "{} `{}` ({}){}",
                    t(lang, "provider padrao", "default provider"),
                    p.nome,
                    p.tipo,
                    if p.keyless {
                        t(lang, " · daemon local respondendo", " · local daemon answering")
                    } else {
                        ""
                    }
                ),
                None,
            ),
            None => (
                Semaforo::Ok,
                format!(
                    "{} {}; {}",
                    c.provedores.len(),
                    t(lang, "provider(s) configurado(s)", "provider(s) configured"),
                    t(
                        lang,
                        "o padrao e decidido no boot (agent.default_provider ausente)",
                        "the default is decided at boot (agent.default_provider unset)"
                    )
                ),
                None,
            ),
        }
    };
    out.push(Linha::nova("provider.default", status, detalhe, passo));

    out
}

/// `error` → 69; `warning` so conta sob `--strict` (2); o resto e 0.
pub(crate) fn exit_code(linhas: &[Linha], strict: bool) -> i32 {
    if linhas.iter().any(|l| l.status == Semaforo::Error) {
        EX_UNAVAILABLE
    } else if strict && linhas.iter().any(|l| l.status == Semaforo::Warning) {
        EX_CONFIG
    } else {
        EX_OK
    }
}

/// O agregado, no vocabulario do `/api/diagnostics`.
pub(crate) fn agregado(linhas: &[Linha]) -> &'static str {
    if linhas.iter().any(|l| l.status == Semaforo::Error) {
        "error"
    } else if linhas.iter().any(|l| l.status == Semaforo::Warning) {
        "warning"
    } else {
        "ok"
    }
}
