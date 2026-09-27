//! A colheita do `doctor whatsapp`: disco, config, processo e o
//! `/api/diagnostics` do gateway vivo — uma vez, antes da tabela, pelas MESMAS
//! fontes que o `status`, o boot e o console usam.
//!
//! A leitura pura da config e do gateway (`doctor::fatos_da_config`); o que
//! a CLI acrescenta e o I/O que so faz sentido neste processo: o probe TCP
//! dos daemons locais keyless e a pergunta ao gateway vivo por HTTP.

use std::collections::HashMap;

use garraia_channels::whatsapp_linked::KeyOrigin;
use garraia_channels::whatsapp_linked::health::{BridgeView, DiskFacts, LinkHealth, classify};
use garraia_config::AppConfig;
use garraia_gateway::bootstrap::whatsapp_linked_doctor as doctor;
use garraia_gateway::bootstrap::whatsapp_linked_doctor::{
    Chave, ConfigFatos, Fatos, Gateway, LinhaViva, Sessao,
};

use crate::whatsapp::Context;

/// Colhe os fatos das mesmas fontes que o `status`, o boot e o console usam.
pub(crate) fn colher(ctx: &Context) -> Fatos {
    let bridge_dir = ctx.bridge_dir();
    let sessao = match ctx.store() {
        Err(e) => Sessao::Indisponivel(e.to_string()),
        Ok(store) => {
            let facts = DiskFacts::read(&store, &bridge_dir);
            let saude = classify(&facts, BridgeView::Unknown);
            // Derivar a chave custa PBKDF2 (600k iteracoes): so quando ha
            // sessao para abrir — e e a unica pergunta que o console nao faz.
            let chave = (saude != LinkHealth::NotLinked).then(|| match ctx.key() {
                Ok(key) => Chave {
                    do_cofre: key.origin() == KeyOrigin::VaultPassphrase,
                    legivel: store.load(&key).is_ok(),
                },
                Err(_) => Chave {
                    do_cofre: false,
                    legivel: false,
                },
            });
            Sessao::Lida { saude, chave }
        }
    };
    let config = ctx.loader.as_ref().and_then(|l| l.load().ok());
    // A MESMA credencial que o `garra status` manda (#1045/#1261): com
    // `gateway.api_key` configurada, `/api/*` exige `Authorization: Bearer`
    // — so `/api/health` e `/api/capabilities` seguem abertas. Sem isto o
    // doctor responderia "nao respondeu" contra o proprio gateway do usuario.
    let bearer = config
        .as_ref()
        .and_then(|c| c.gateway.api_key_normalizada());
    let config = config.as_ref().map(fatos_da_config);
    let (host, porta) = garraia_config::bind::endereco_do_cliente();
    let daemon = crate::doctor::check_daemon(&host, porta);
    let (ao_vivo, recusou_credencial) = if daemon.port_listening {
        diagnostics_ao_vivo(&host, porta, bearer)
    } else {
        (Vec::new(), false)
    };
    Fatos {
        sessao,
        bridge_dir,
        config,
        gateway: Gateway {
            pid: daemon.pid.filter(|_| daemon.process_alive),
            ouvindo: daemon.port_listening,
            host,
            porta,
            ao_vivo,
            recusou_credencial,
        },
    }
}

/// O recorte da config que a tabela precisa — pela leitura pura do gateway,
/// mais o que so a CLI faz: o probe TCP dos daemons locais keyless. O
/// gateway nao sonda (julga o provider pelo que ele mesmo registrou), entao
/// `alcancavel` vem `None` de la e e preenchido aqui, por nome.
fn fatos_da_config(config: &AppConfig) -> ConfigFatos {
    let mut fatos = doctor::fatos_da_config(config);
    let sondas: HashMap<String, Option<bool>> = crate::doctor::collect_provider_checks(config)
        .into_iter()
        .map(|p| {
            let alcancavel = p
                .probe
                .as_ref()
                .map(|r| matches!(r, crate::doctor::ProbeResult::Reachable));
            (p.name, alcancavel)
        })
        .collect();
    for p in &mut fatos.provedores {
        p.alcancavel = sondas.get(&p.nome).copied().flatten();
    }
    fatos
}

/// `GET /api/diagnostics` do gateway local, pelo MESMO guard SSRF do probe
/// do doctor (URL vetada, IPs pinados, sem redirect, corpo limitado), com a
/// credencial do gateway quando a config a tem. Devolve as linhas e se o
/// gateway **recusou a credencial** (401) — os dois "nao sei" sao diferentes
/// para quem le: um pede o log, o outro pede a chave no ambiente da CLI.
/// Qualquer outra falha e `Vec::new()`: a tabela diz que nao sabe, nao que
/// caiu.
fn diagnostics_ao_vivo(host: &str, porta: u16, bearer: Option<&str>) -> (Vec<LinhaViva>, bool) {
    let host_url = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let url = format!("http://{host_url}:{porta}/api/diagnostics");
    let politica = crate::doctor::daemon_probe_policy();
    let Ok(vetted) = garraia_common::ssrf::vet_url(&url, &politica) else {
        return (Vec::new(), false);
    };
    let Ok(client) = garraia_common::ssrf::pinned_client(&vetted, &politica) else {
        return (Vec::new(), false);
    };
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return (Vec::new(), false);
    };
    let (corpo, recusou): (Option<serde_json::Value>, bool) = rt.block_on(async {
        let mut pedido = client.get(vetted.url.clone());
        if let Some(chave) = bearer {
            pedido = pedido.bearer_auth(chave);
        }
        let Ok(resp) = pedido.send().await else {
            return (None, false);
        };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return (None, true);
        }
        if !resp.status().is_success() {
            return (None, false);
        }
        let Ok(bytes) = garraia_common::ssrf::read_capped(resp, 256 * 1024).await else {
            return (None, false);
        };
        (serde_json::from_slice(&bytes).ok(), false)
    });
    let linhas = corpo
        .and_then(|v| v.get("checks")?.as_array().cloned())
        .map(|checks| {
            checks
                .iter()
                .filter_map(|c| {
                    Some(LinhaViva {
                        id: c.get("id")?.as_str()?.to_string(),
                        status: c.get("status")?.as_str()?.to_string(),
                        detail: c
                            .get("detail")
                            .and_then(|d| d.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        next_step: c
                            .get("next_step")
                            .and_then(|n| n.as_str())
                            .map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    (linhas, recusou)
}
