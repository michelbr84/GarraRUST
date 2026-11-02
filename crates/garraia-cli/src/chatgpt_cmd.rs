//! `garra chatgpt` — Sign in with ChatGPT (provider `openai-codex`).
//!
//! - `login`: abre o navegador do sistema no fluxo oficial da OpenAI e espera
//!   o callback loopback; `--padrao` grava a entrada `llm.chatgpt`
//!   (gpt-6.1-sol, reasoning `high`) e a torna o provider padrao.
//! - `logout`: revoga a sessao e apaga os tokens locais.
//! - `status`, `modelos`, `usar`, `configurar`, `testar`.
//!
//! O consentimento e sempre da pessoa no site da OpenAI. Nada aqui imprime
//! token, codigo de autorizacao ou o `id_token_hint`.

use anyhow::Result;
use garraia_agents::openai_codex::{
    ArmazemDeCredenciais, ContaChatGpt, ESFORCO_PADRAO, MODELO_PADRAO, OpcoesDeLogin,
    OpenAiCodexProvider, esforco_normalizado, login, velocidade_normalizada,
};
use garraia_agents::providers::LlmProvider;
use garraia_config::ConfigLoader;
use garraia_config::chatgpt::{AjusteDoChatGpt, aplicar, entrada_existente};

fn armazem() -> ArmazemDeCredenciais {
    ArmazemDeCredenciais::new(ConfigLoader::openai_codex_credentials_dir())
}

/// Abre a URL no navegador do sistema. Falhar aqui nao e erro: a URL tambem
/// e impressa para a pessoa abrir a mao.
fn abrir_navegador(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).status();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .status();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).status();
    r.is_ok_and(|s| s.success())
}

fn descrever(conta: &ContaChatGpt) -> String {
    let quem = conta
        .email
        .clone()
        .unwrap_or_else(|| conta.client_id.clone());
    let plano = if conta.plano_habilitado {
        "uso do plano autorizado"
    } else {
        "uso do plano NAO autorizado"
    };
    let sessao = if conta.conectado {
        "conectada"
    } else {
        "desconectada"
    };
    format!("{quem} ({sessao}, {plano})")
}

/// Le a config, aplica o ajuste e grava (0600). Nao devolve nada derivado da
/// config gravada: o que e impresso depois e texto fixo (a config pode ter
/// credenciais de outros providers; o CodeQL trata o fluxo como sensivel).
fn gravar_config(ajuste: &AjusteDoChatGpt<'_>) -> Result<()> {
    let loader = ConfigLoader::new()?;
    loader.ensure_dirs()?;
    let mut config = loader.load().map_err(|e| {
        anyhow::anyhow!("o config.yml atual nao pode ser lido ({e}); corrija antes de gravar")
    })?;
    aplicar(&mut config, ajuste);
    loader.save(&config)?;
    Ok(())
}

pub async fn run_login(
    nova_conta: bool,
    porta: Option<u16>,
    sem_navegador: bool,
    padrao: bool,
) -> Result<i32> {
    let a = armazem();
    let andamento = login::iniciar(
        &a,
        OpcoesDeLogin {
            nova_conta,
            porta,
            ..Default::default()
        },
    )
    .await?;
    println!("Entrar com o ChatGPT (fluxo oficial Sign in with ChatGPT da OpenAI).");
    if andamento.registro_novo {
        println!(
            "Primeiro login desta conta neste computador: o GarraIA sera registrado na sua conta."
        );
    }
    if sem_navegador || !abrir_navegador(&andamento.url) {
        println!(
            "\nAbra este endereco no navegador deste computador:\n\n  {}\n",
            andamento.url
        );
    } else {
        println!(
            "Abri o navegador. Se nao abriu, use este endereco:\n\n  {}\n",
            andamento.url
        );
    }
    println!(
        "Aguardando a volta do navegador em {} ...",
        andamento.redirect_uri
    );
    match andamento.concluir().await {
        Ok(r) => {
            println!("\nConectado: {}", descrever(&r.conta));
            if !r.conta.plano_habilitado {
                println!(
                    "A conta entrou, mas sem autorizar o uso do plano do ChatGPT. Rode `garra chatgpt login` de novo e autorize o uso do plano."
                );
                return Ok(1);
            }
            if r.registro_novo {
                println!(
                    "\nVoce esta usando seu plano do ChatGPT no GarraIA. Acompanhe o uso em ChatGPT > Configuracoes > Uso."
                );
            }
            if padrao {
                gravar_config(&AjusteDoChatGpt {
                    tornar_padrao: true,
                    ..Default::default()
                })?;
                println!(
                    "config.yml: entrada openai-codex ({MODELO_PADRAO}, reasoning {ESFORCO_PADRAO}) gravada e definida como provider padrao. Reinicie o gateway para aplicar."
                );
            } else {
                println!("Para usar como padrao: `garra chatgpt configurar --padrao`.");
            }
            Ok(0)
        }
        Err(e) => {
            eprintln!("\nO login nao foi concluido: {e}");
            Ok(1)
        }
    }
}

pub async fn run_logout() -> Result<i32> {
    let a = armazem();
    let Some(conta) = a.conta_ativa() else {
        println!("Nenhuma conta do ChatGPT conectada.");
        return Ok(0);
    };
    let confirmada = a.sair().await?;
    println!(
        "Saiu de {}. Tokens locais apagados.",
        conta.email.unwrap_or(conta.client_id)
    );
    if !confirmada {
        println!(
            "Aviso: a revogacao remota nao foi confirmada. Se quiser garantir, desconecte o GarraIA em ChatGPT > Configuracoes > Apps conectados."
        );
    }
    Ok(0)
}

pub fn run_status(json: bool) -> Result<i32> {
    let a = armazem();
    let ativa = a.conta_ativa();
    let config = ConfigLoader::new().ok().and_then(|l| l.load().ok());
    let entrada = config
        .as_ref()
        .and_then(|c| entrada_existente(c).map(|k| (k.clone(), c.llm[&k].clone())));
    let padrao = config
        .as_ref()
        .and_then(|c| c.agent.default_provider.clone());
    if json {
        let v = serde_json::json!({
            "conta_ativa": ativa,
            "contas": a.contas(),
            "config": entrada.as_ref().map(|(k, e)| serde_json::json!({
                "entrada": k,
                "modelo": e.model,
                "reasoning_effort": e.extra.get("reasoning_effort"),
                "padrao": padrao.as_deref() == Some(k.as_str()),
            })),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(
            if ativa
                .as_ref()
                .is_some_and(|c| c.conectado && c.plano_habilitado)
            {
                0
            } else {
                1
            },
        );
    }
    match &ativa {
        Some(c) => {
            println!("Conta em uso: {}", descrever(c));
            if let Some(v) = &c.token_vence_em {
                println!("Access token vence em: {v} (renovado sozinho)");
            }
        }
        None => println!("Nenhuma conta do ChatGPT conectada. Rode `garra chatgpt login`."),
    }
    let outras: Vec<String> = a
        .contas()
        .into_iter()
        .filter(|c| Some(&c.client_id) != ativa.as_ref().map(|a| &a.client_id))
        .map(|c| descrever(&c))
        .collect();
    if !outras.is_empty() {
        println!("Outras contas neste computador: {}", outras.join("; "));
    }
    match &entrada {
        Some((k, e)) => println!(
            "config.yml: llm.{k} — modelo {}, reasoning {}, velocidade {}{}",
            e.model.as_deref().unwrap_or(MODELO_PADRAO),
            e.extra
                .get("reasoning_effort")
                .and_then(|v| v.as_str())
                .unwrap_or(ESFORCO_PADRAO),
            e.extra
                .get("speed")
                .and_then(|v| v.as_str())
                .unwrap_or("thinking"),
            if padrao.as_deref() == Some(k.as_str()) {
                " (provider padrao)"
            } else {
                ""
            }
        ),
        None => println!(
            "config.yml: nenhuma entrada openai-codex (use `garra chatgpt configurar --padrao`)."
        ),
    }
    Ok(
        if ativa.is_some_and(|c| c.conectado && c.plano_habilitado) {
            0
        } else {
            1
        },
    )
}

pub async fn run_modelos() -> Result<i32> {
    let p = OpenAiCodexProvider::new(ConfigLoader::openai_codex_credentials_dir(), None, None);
    let modelos = p.listar_modelos().await?;
    if modelos.is_empty() {
        println!("O catalogo desta conta veio vazio.");
        return Ok(1);
    }
    println!("Modelos disponiveis para a conta (catalogo; a inferencia confirma o acesso):");
    for m in modelos {
        let marca = if m.slug == MODELO_PADRAO {
            "  ← preferencial"
        } else {
            ""
        };
        println!(
            "  {}  {}{marca}",
            m.slug,
            m.display_name.clone().unwrap_or_default()
        );
        // Niveis do catalogo que a Responses API publica aceita; o resto
        // (o `ultra`, que so existe no Codex) aparece marcado.
        let niveis: Vec<String> = m
            .esforcos
            .iter()
            .map(|e| {
                if esforco_normalizado(e).is_some() {
                    e.clone()
                } else {
                    format!("{e} (so no Codex)")
                }
            })
            .collect();
        let mut velocidades = vec!["thinking".to_string()];
        velocidades.extend(m.velocidades.iter().cloned());
        println!(
            "      raciocinio: {} | velocidade: {}",
            if niveis.is_empty() {
                "-".into()
            } else {
                niveis.join(", ")
            },
            velocidades.join(", ")
        );
    }
    Ok(0)
}

pub fn run_usar(conta: &str) -> Result<i32> {
    let a = armazem();
    let alvo = a
        .contas()
        .into_iter()
        .find(|c| c.client_id == conta || c.email.as_deref() == Some(conta));
    match alvo {
        Some(c) => {
            a.ativar(&c.client_id)?;
            println!("Conta em uso: {}", descrever(&c));
            Ok(0)
        }
        None => {
            eprintln!(
                "Conta '{conta}' nao encontrada. Contas registradas: veja `garra chatgpt status`."
            );
            Ok(1)
        }
    }
}

pub fn run_configurar(
    modelo: Option<&str>,
    esforco: Option<&str>,
    velocidade: Option<&str>,
    padrao: bool,
) -> Result<i32> {
    if let Some(e) = esforco
        && esforco_normalizado(e).is_none()
    {
        eprintln!(
            "reasoning effort '{e}' nao e aceito pela Responses API. Use: minimal, low, medium, high, xhigh ou max (`ultra` so existe no Codex)."
        );
        return Ok(2);
    }
    if let Some(v) = velocidade
        && velocidade_normalizada(v).is_none()
    {
        eprintln!("velocidade invalida: '{v}'. Use: thinking, fast ou ultrafast.");
        return Ok(2);
    }
    gravar_config(&AjusteDoChatGpt {
        modelo,
        esforco,
        velocidade,
        tornar_padrao: padrao,
    })?;
    println!(
        "config.yml: entrada openai-codex atualizada{}. Reinicie o gateway para aplicar.",
        if padrao {
            " e definido como provider padrao"
        } else {
            ""
        }
    );
    Ok(0)
}

/// Uma inferencia curta e real (nao e mock): prova login, autorizacao do
/// plano, modelo, esforco, velocidade e streaming de uma vez, e mostra o
/// `service_tier` que a OpenAI serviu de fato.
pub async fn run_testar(
    modelo: Option<String>,
    esforco: Option<String>,
    velocidade: Option<String>,
) -> Result<i32> {
    let config = ConfigLoader::new().ok().and_then(|l| l.load().ok());
    let entrada = config
        .as_ref()
        .and_then(|c| entrada_existente(c).map(|k| c.llm[&k].clone()));
    let extra = |chave: &str| {
        entrada.as_ref().and_then(|e| {
            e.extra
                .get(chave)
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
    };
    let modelo = modelo.or_else(|| entrada.as_ref().and_then(|e| e.model.clone()));
    let esforco = esforco.or_else(|| extra("reasoning_effort"));
    let velocidade = velocidade.or_else(|| extra("speed"));
    let p = OpenAiCodexProvider::new(
        ConfigLoader::openai_codex_credentials_dir(),
        modelo,
        esforco,
    )
    .with_velocidade(velocidade);
    println!(
        "Teste real: modelo {}, reasoning.effort={}, velocidade {}, streaming.",
        p.configured_model().unwrap_or(MODELO_PADRAO),
        p.esforco(),
        p.velocidade()
    );
    match p.testar(None).await {
        Ok(r) => {
            println!(
                "Uso: {} tokens de entrada, {} de saida.",
                r.tokens_entrada, r.tokens_saida
            );
            println!(
                "Resposta: {:?} ({} trechos em streaming, {} ms). Login, plano, modelo e streaming OK.",
                r.texto, r.trechos_em_streaming, r.ms
            );
            println!(
                "service_tier servido pela OpenAI: {}",
                r.tier_servido.as_deref().unwrap_or("(nao informado)")
            );
            if p.velocidade() != "thinking"
                && r.tier_servido.as_deref() != Some("priority")
                && r.tier_servido.as_deref() != Some("ultrafast")
            {
                println!(
                    "Aviso: o modo {} foi pedido, mas a OpenAI serviu o tier normal nesta conta/fluxo.",
                    p.velocidade()
                );
            }
            Ok(0)
        }
        Err(e) => {
            eprintln!("Falhou: {e}");
            Ok(1)
        }
    }
}
