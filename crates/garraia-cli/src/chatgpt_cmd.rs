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

use std::time::Instant;

use anyhow::Result;
use futures::StreamExt;
use garraia_agents::openai_codex::{
    ArmazemDeCredenciais, ContaChatGpt, ESFORCO_PADRAO, MODELO_PADRAO, OpcoesDeLogin,
    OpenAiCodexProvider, esforco_normalizado, login,
};
use garraia_agents::providers::{
    ChatMessage, ChatRole, LlmProvider, LlmRequest, MessagePart, StreamEvent,
};
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

/// Le a config, aplica o ajuste e grava (0600). Devolve a chave da entrada.
fn gravar_config(ajuste: &AjusteDoChatGpt<'_>) -> Result<String> {
    let loader = ConfigLoader::new()?;
    loader.ensure_dirs()?;
    let mut config = loader.load().map_err(|e| {
        anyhow::anyhow!("o config.yml atual nao pode ser lido ({e}); corrija antes de gravar")
    })?;
    let chave = aplicar(&mut config, ajuste);
    loader.save(&config)?;
    Ok(chave)
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
                let chave = gravar_config(&AjusteDoChatGpt {
                    tornar_padrao: true,
                    ..Default::default()
                })?;
                println!(
                    "config.yml: llm.{chave} (openai-codex, {MODELO_PADRAO}, reasoning {ESFORCO_PADRAO}) agora e o provider padrao. Reinicie o gateway para aplicar."
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
            "config.yml: llm.{k} — modelo {}, reasoning {}{}",
            e.model.as_deref().unwrap_or(MODELO_PADRAO),
            e.extra
                .get("reasoning_effort")
                .and_then(|v| v.as_str())
                .unwrap_or(ESFORCO_PADRAO),
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
            m.display_name.unwrap_or_default()
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

pub fn run_configurar(modelo: Option<&str>, esforco: Option<&str>, padrao: bool) -> Result<i32> {
    if let Some(e) = esforco
        && esforco_normalizado(e).is_none()
    {
        eprintln!(
            "reasoning effort invalido: '{e}'. Use: none, minimal, low, medium, high, xhigh ou max."
        );
        return Ok(2);
    }
    let chave = gravar_config(&AjusteDoChatGpt {
        modelo,
        esforco,
        tornar_padrao: padrao,
    })?;
    println!(
        "config.yml: llm.{chave} atualizado{}. Reinicie o gateway para aplicar.",
        if padrao {
            " e definido como provider padrao"
        } else {
            ""
        }
    );
    Ok(0)
}

/// Uma inferencia curta e real (nao e mock): prova login, autorizacao do
/// plano, modelo, esforco e streaming de uma vez.
pub async fn run_testar(modelo: Option<String>, esforco: Option<String>) -> Result<i32> {
    let config = ConfigLoader::new().ok().and_then(|l| l.load().ok());
    let entrada = config
        .as_ref()
        .and_then(|c| entrada_existente(c).map(|k| c.llm[&k].clone()));
    let modelo = modelo.or_else(|| entrada.as_ref().and_then(|e| e.model.clone()));
    let esforco = esforco.or_else(|| {
        entrada.as_ref().and_then(|e| {
            e.extra
                .get("reasoning_effort")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
    });
    let p = OpenAiCodexProvider::new(
        ConfigLoader::openai_codex_credentials_dir(),
        modelo,
        esforco,
    );
    let modelo_usado = p.configured_model().unwrap_or(MODELO_PADRAO).to_string();
    println!(
        "Teste real: modelo {modelo_usado}, reasoning.effort={}, streaming.",
        p.esforco()
    );
    let pedido = LlmRequest {
        model: String::new(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text("Responda exatamente: ok".into()),
        }],
        system: None,
        max_tokens: None,
        temperature: None,
        tools: Vec::new(),
    };
    let inicio = Instant::now();
    let mut stream = match p.stream_complete(&pedido).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Falhou antes do streaming: {e}");
            return Ok(1);
        }
    };
    let mut deltas = 0usize;
    let mut texto = String::new();
    let mut concluido = false;
    while let Some(ev) = stream.next().await {
        match ev {
            Ok(StreamEvent::TextDelta(t)) => {
                deltas += 1;
                texto.push_str(&t);
            }
            Ok(StreamEvent::MessageDelta { usage, .. }) => {
                if let Some(u) = usage {
                    println!(
                        "Uso: {} tokens de entrada, {} de saida.",
                        u.input_tokens, u.output_tokens
                    );
                }
            }
            Ok(StreamEvent::MessageStop) => concluido = true,
            Ok(_) => {}
            Err(e) => {
                eprintln!("Falhou durante o streaming: {e}");
                return Ok(1);
            }
        }
    }
    if !concluido {
        eprintln!("O stream terminou sem response.completed.");
        return Ok(1);
    }
    println!(
        "Resposta: {:?} ({deltas} trechos em streaming, {} ms). Login, plano, modelo e streaming OK.",
        texto.trim(),
        inicio.elapsed().as_millis()
    );
    Ok(0)
}
