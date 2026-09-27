//! #1438: o registro de confiabilidade, sem runtime — contagem por
//! ferramenta e por desfecho, timeout separado de erro, baldes de latencia,
//! tetos de cardinalidade, MCP, canal, tendencia e o snapshot sem PII.

use super::*;
use std::sync::Arc;

fn chamada(nome: &str, desfecho: Desfecho, ms: u64) -> Chamada<'_> {
    Chamada {
        nome,
        conhecida: true,
        desfecho,
        duracao: Duration::from_millis(ms),
        abriu_breaker: false,
    }
}

fn linha<'a>(s: &'a Snapshot, tool: &str) -> &'a ToolSnapshot {
    s.tools
        .iter()
        .find(|t| t.tool == tool)
        .unwrap_or_else(|| panic!("sem linha para {tool}: {s:?}"))
}

fn segundos(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn conta_cada_desfecho_por_ferramenta() {
    let obs = Observabilidade::new();
    let t0 = Instant::now();
    obs.registrar(chamada("file_read", Desfecho::Sucesso, 5), t0);
    obs.registrar(chamada("file_read", Desfecho::Sucesso, 7), t0);
    obs.registrar(chamada("file_read", Desfecho::Erro, 3), t0);
    obs.registrar(chamada("repo_search", Desfecho::Timeout, 30_000), t0);
    obs.registrar(chamada("bash", Desfecho::NegadaPelaPolitica, 0), t0);
    obs.registrar(chamada("telegram_send", Desfecho::Indisponivel, 0), t0);
    obs.registrar(chamada("repo_search", Desfecho::RecusadaPeloBreaker, 0), t0);
    obs.registrar(chamada("bash", Desfecho::AguardandoConfirmacao, 2), t0);

    let s = obs.snapshot(t0);
    let fr = linha(&s, "file_read");
    assert_eq!(fr.stats.calls, 3);
    assert_eq!(fr.stats.outcomes["success"], 2);
    assert_eq!(fr.stats.outcomes["error"], 1);
    assert_eq!(
        fr.stats.outcomes["timeout"], 0,
        "todo desfecho aparece, mesmo zerado"
    );
    assert_eq!(fr.stats.outcomes.len(), Desfecho::TODOS.len());
    let rs = linha(&s, "repo_search");
    assert_eq!(rs.stats.outcomes["timeout"], 1);
    assert_eq!(rs.stats.outcomes["breaker_refused"], 1);
    let bash = linha(&s, "bash");
    assert_eq!(bash.stats.outcomes["denied_by_policy"], 1);
    assert_eq!(bash.stats.outcomes["awaiting_confirmation"], 1);
    assert_eq!(linha(&s, "telegram_send").stats.outcomes["unavailable"], 1);

    let nomes: Vec<&str> = s.tools.iter().map(|t| t.tool.as_str()).collect();
    assert_eq!(
        nomes,
        ["bash", "file_read", "repo_search", "telegram_send"],
        "ordem lexica"
    );
    assert_eq!(s.tools_total.calls, 8);
    assert_eq!(s.tools_total.outcomes["success"], 2);
    assert_eq!(s.tools_total.outcomes["denied_by_policy"], 1);
    assert_eq!(
        s.tools_total.outcomes.values().sum::<u64>(),
        s.tools_total.calls,
        "chamadas = soma dos desfechos"
    );
}

#[test]
fn timeout_nao_e_erro() {
    use crate::tools::breaker::TIMEOUT_PREFIXO;
    assert_eq!(
        Desfecho::da_saida(&ToolOutput::success("ok")),
        Desfecho::Sucesso
    );
    assert_eq!(
        Desfecho::da_saida(&ToolOutput::error("deu ruim")),
        Desfecho::Erro
    );
    assert_eq!(
        Desfecho::da_saida(&ToolOutput::error(format!("{TIMEOUT_PREFIXO}lenta"))),
        Desfecho::Timeout
    );
    assert_eq!(
        Desfecho::da_saida(&ToolOutput::error(
            crate::tools::file_jail::NO_ROOTS_MESSAGE
        )),
        Desfecho::Erro,
        "falha deterministica do breaker e erro, nao timeout"
    );
    assert_eq!(
        Desfecho::da_saida(&ToolOutput::confirmation_request("confirme")),
        Desfecho::AguardandoConfirmacao,
        "o pedido de confirmacao sai com is_error e nao e falha"
    );

    let obs = Observabilidade::new();
    let t0 = Instant::now();
    obs.registrar(chamada("lenta", Desfecho::Timeout, 30_000), t0);
    obs.registrar(chamada("lenta", Desfecho::Sucesso, 100), t0);
    obs.registrar(chamada("lenta", Desfecho::Sucesso, 100), t0);
    obs.registrar(chamada("lenta", Desfecho::Erro, 100), t0);
    let s = obs.snapshot(t0);
    let l = linha(&s, "lenta");
    assert_eq!(l.stats.outcomes["timeout"], 1);
    assert_eq!(l.stats.outcomes["error"], 1, "o timeout nao entra em erro");
    assert_eq!(l.stats.success_rate, Some(0.5));
    assert_eq!(l.stats.error_rate, Some(0.25));
    assert_eq!(l.stats.timeout_rate, Some(0.25));
}

#[test]
fn taxas_ignoram_recusas_e_arredondam() {
    let obs = Observabilidade::new();
    let t0 = Instant::now();
    obs.registrar(chamada("so_negada", Desfecho::NegadaPelaPolitica, 0), t0);
    obs.registrar(chamada("so_negada", Desfecho::AguardandoConfirmacao, 1), t0);
    obs.registrar(chamada("um_terco", Desfecho::Sucesso, 1), t0);
    obs.registrar(chamada("um_terco", Desfecho::Erro, 1), t0);
    obs.registrar(chamada("um_terco", Desfecho::Erro, 1), t0);
    obs.registrar(chamada("um_terco", Desfecho::RecusadaPeloBreaker, 0), t0);
    let s = obs.snapshot(t0);
    let negada = linha(&s, "so_negada");
    assert_eq!(
        negada.stats.success_rate, None,
        "nada executou com veredito"
    );
    assert_eq!(negada.stats.error_rate, None);
    let terco = linha(&s, "um_terco");
    assert_eq!(terco.stats.success_rate, Some(0.3333));
    assert_eq!(terco.stats.error_rate, Some(0.6667));
    assert_eq!(terco.stats.timeout_rate, Some(0.0));
}

#[test]
fn latencia_em_baldes_fixos_com_quantis_aproximados() {
    let mut h = Histograma::default();
    for _ in 0..90 {
        h.registrar(Duration::from_millis(5));
    }
    for _ in 0..10 {
        h.registrar(Duration::from_millis(3_000));
    }
    assert_eq!(h.contagem(), 100);
    assert_eq!(h.quantil(0.5), Some(10), "limite superior do balde <=10ms");
    assert_eq!(
        h.quantil(0.95),
        Some(3_000),
        "o balde <=5000 e cortado pelo maximo observado"
    );
    assert_eq!(h.quantil(1.0), Some(3_000));
    let s = h.snapshot();
    assert_eq!(s.count, 100);
    assert_eq!(s.max, Some(3_000));
    assert_eq!(s.avg, Some((90 * 5 + 10 * 3_000) / 100));
    assert_eq!(s.p50, Some(10));
    assert_eq!(s.p95, Some(3_000));
    assert_eq!(s.buckets.len(), LIMITES_DE_LATENCIA_MS.len() + 1);
    assert_eq!(
        s.buckets[0],
        BucketSnapshot {
            le: Some(10),
            count: 90
        }
    );
    assert_eq!(
        s.buckets[7],
        BucketSnapshot {
            le: Some(5_000),
            count: 10
        }
    );
    assert_eq!(
        s.buckets.last(),
        Some(&BucketSnapshot { le: None, count: 0 })
    );

    // Alem do ultimo limite vai para o `+Inf`, e o quantil vira o maximo.
    let mut lento = Histograma::default();
    lento.registrar(segundos(120));
    assert_eq!(lento.quantil(0.95), Some(120_000));
    assert_eq!(lento.snapshot().buckets.last().map(|b| b.count), Some(1));

    // O limite e inclusivo: 10ms cai no primeiro balde.
    let mut borda = Histograma::default();
    borda.registrar(Duration::from_millis(10));
    assert_eq!(borda.snapshot().buckets[0].count, 1);

    let vazio = Histograma::default();
    assert_eq!(vazio.quantil(0.5), None);
    assert_eq!(vazio.snapshot().avg, None);
    assert_eq!(vazio.snapshot().max, None);
}

#[test]
fn a_latencia_nao_guarda_amostra_e_soma_exato() {
    let mut a = Histograma::default();
    let mut b = Histograma::default();
    let mut junto = Histograma::default();
    for i in 0..50_000u64 {
        let d = Duration::from_millis(i % 70_000);
        if i % 2 == 0 {
            a.registrar(d);
        } else {
            b.registrar(d);
        }
        junto.registrar(d);
    }
    let s = junto.snapshot();
    assert_eq!(s.buckets.len(), LIMITES_DE_LATENCIA_MS.len() + 1);
    assert_eq!(s.buckets.iter().map(|x| x.count).sum::<u64>(), 50_000);
    a.somar(&b);
    assert_eq!(
        a, junto,
        "a soma de dois histogramas e o histograma do todo"
    );
}

#[test]
fn recusa_nao_entra_na_latencia() {
    for d in [
        Desfecho::Sucesso,
        Desfecho::Erro,
        Desfecho::Timeout,
        Desfecho::AguardandoConfirmacao,
    ] {
        assert!(d.executou(), "{d:?}");
    }
    for d in [
        Desfecho::NegadaPelaPolitica,
        Desfecho::Indisponivel,
        Desfecho::RecusadaPeloBreaker,
    ] {
        assert!(!d.executou(), "{d:?}");
    }

    let obs = Observabilidade::new();
    let t0 = Instant::now();
    obs.registrar(chamada("bash", Desfecho::NegadaPelaPolitica, 9_999), t0);
    obs.registrar(chamada("bash", Desfecho::RecusadaPeloBreaker, 9_999), t0);
    obs.registrar(chamada("bash", Desfecho::Indisponivel, 9_999), t0);
    obs.registrar(chamada("bash", Desfecho::Sucesso, 20), t0);
    let s = obs.snapshot(t0);
    let b = linha(&s, "bash");
    assert_eq!(b.stats.calls, 4);
    assert_eq!(b.stats.latency_ms.count, 1);
    assert_eq!(b.stats.latency_ms.max, Some(20));
}

#[test]
fn aberturas_do_breaker_contam_a_parte_das_recusas() {
    let obs = Observabilidade::new();
    let t0 = Instant::now();
    obs.registrar(
        Chamada {
            abriu_breaker: true,
            ..chamada("repo_search", Desfecho::Erro, 5)
        },
        t0,
    );
    obs.registrar(chamada("repo_search", Desfecho::RecusadaPeloBreaker, 0), t0);
    obs.registrar(chamada("repo_search", Desfecho::RecusadaPeloBreaker, 0), t0);
    let s = obs.snapshot(t0);
    let r = linha(&s, "repo_search");
    assert_eq!(r.stats.breaker_opened, 1);
    assert_eq!(r.stats.outcomes["breaker_refused"], 2);
    assert_eq!(s.tools_total.breaker_opened, 1);
}

#[test]
fn ultima_falha_e_relativa_ao_agora_injetado() {
    let obs = Observabilidade::new();
    let t0 = Instant::now();
    obs.registrar(chamada("web_fetch", Desfecho::Erro, 5), t0);
    obs.registrar(
        chamada("web_fetch", Desfecho::Timeout, 5),
        t0 + segundos(10),
    );
    obs.registrar(
        chamada("web_fetch", Desfecho::Sucesso, 5),
        t0 + segundos(20),
    );
    obs.registrar(
        chamada("web_fetch", Desfecho::NegadaPelaPolitica, 0),
        t0 + segundos(30),
    );
    obs.registrar(chamada("file_read", Desfecho::Sucesso, 5), t0);
    let s = obs.snapshot(t0 + segundos(52));
    assert_eq!(
        linha(&s, "web_fetch").stats.last_failure_ago_s,
        Some(42),
        "o timeout e a ultima falha; sucesso e recusa nao sao falha"
    );
    assert_eq!(linha(&s, "file_read").stats.last_failure_ago_s, None);
    assert_eq!(s.tools_total.last_failure_ago_s, Some(42));
}

#[test]
fn teto_de_ferramentas_soma_o_excedente_em_outras() {
    let obs = Observabilidade::com_teto_de_ferramentas(2);
    let t0 = Instant::now();
    obs.registrar(
        Chamada {
            conhecida: false,
            ..chamada("inventada", Desfecho::Erro, 1)
        },
        t0,
    );
    for nome in ["a", "b", "c", "d"] {
        obs.registrar(chamada(nome, Desfecho::Sucesso, 1), t0);
    }
    obs.registrar(chamada("a", Desfecho::Erro, 1), t0);
    let s = obs.snapshot(t0);
    let nomes: Vec<&str> = s.tools.iter().map(|t| t.tool.as_str()).collect();
    assert_eq!(nomes, [OUTRAS, DESCONHECIDA, "a", "b"]);
    assert_eq!(linha(&s, OUTRAS).stats.calls, 2, "c e d");
    assert_eq!(
        linha(&s, "a").stats.calls,
        2,
        "quem ja tem linha continua nela"
    );
    assert_eq!(
        linha(&s, DESCONHECIDA).stats.calls,
        1,
        "os baldes constantes nao gastam o teto"
    );
    assert_eq!(s.tools_total.calls, 6);
}

#[test]
fn nome_comprido_e_truncado() {
    let obs = Observabilidade::new();
    let t0 = Instant::now();
    let comprido = "x".repeat(MAX_NOME * 4);
    obs.registrar(chamada(&comprido, Desfecho::Sucesso, 1), t0);
    let s = obs.snapshot(t0);
    assert_eq!(s.tools.len(), 1);
    assert_eq!(s.tools[0].tool.chars().count(), MAX_NOME);
}

/// O criterio "sem prompt/conteudo": o modelo pode por texto do usuario no
/// NOME da ferramenta que pede. Esse nome nao e de ferramenta registrada, e
/// nunca chega ao snapshot — nem como chave, nem em lugar nenhum do JSON.
#[test]
fn nome_inventado_pelo_modelo_nunca_sai_no_snapshot() {
    const PROMPT: &str = "PROMPT-SECRETO-que-o-usuario-digitou-5511999998888";
    let obs = Observabilidade::new();
    let t0 = Instant::now();
    for desfecho in [Desfecho::Erro, Desfecho::NegadaPelaPolitica] {
        obs.registrar(
            Chamada {
                nome: PROMPT,
                conhecida: false,
                desfecho,
                duracao: Duration::from_millis(3),
                abriu_breaker: false,
            },
            t0,
        );
    }
    let s = obs.snapshot(t0);
    let json = serde_json::to_string(&s).expect("serializa");
    assert!(!json.contains("PROMPT-SECRETO"), "{json}");
    assert!(!json.contains("5511999998888"), "{json}");
    assert_eq!(linha(&s, DESCONHECIDA).stats.calls, 2);
}

#[test]
fn mcp_conta_queda_por_transicao_e_cada_reconexao() {
    let obs = Observabilidade::new();
    let t0 = Instant::now();
    obs.registrar_transporte_mcp("memoria", true);
    obs.registrar_transporte_mcp("memoria", false);
    obs.registrar_transporte_mcp("memoria", false);
    obs.registrar_reconexao_mcp("memoria", false, t0);
    obs.registrar_reconexao_mcp("memoria", true, t0 + segundos(10));
    obs.registrar_transporte_mcp("memoria", true);
    obs.registrar_transporte_mcp("memoria", false);
    obs.registrar_transporte_mcp("filesystem", true);

    let s = obs.snapshot(t0 + segundos(70));
    let nomes: Vec<&str> = s.mcp.iter().map(|m| m.server.as_str()).collect();
    assert_eq!(nomes, ["filesystem", "memoria"]);
    let m = &s.mcp[1];
    assert_eq!(
        m.drops, 2,
        "o tick seguinte do mesmo transporte morto nao e outra queda"
    );
    assert_eq!(m.reconnect_attempts, 2);
    assert_eq!(m.reconnects_ok, 1);
    assert_eq!(m.reconnects_failed, 1);
    assert_eq!(m.last_reconnect_ago_s, Some(60));
    assert_eq!(s.mcp[0].drops, 0);
    assert_eq!(s.mcp[0].last_reconnect_ago_s, None);
}

#[test]
fn teto_de_servidores_mcp() {
    let obs = Observabilidade::new();
    let t0 = Instant::now();
    for i in 0..(MAX_SERVIDORES_MCP + 6) {
        obs.registrar_reconexao_mcp(&format!("srv{i:03}"), true, t0);
    }
    let s = obs.snapshot(t0);
    assert_eq!(s.mcp.len(), MAX_SERVIDORES_MCP + 1);
    let outras = s
        .mcp
        .iter()
        .find(|m| m.server == OUTRAS)
        .expect("o excedente soma num balde");
    assert_eq!(outras.reconnect_attempts, 6);
}

#[test]
fn canal_conta_conexoes_quedas_e_reconexoes() {
    let obs = Observabilidade::new();
    let t0 = Instant::now();
    let canal = "whatsapp_linked";
    obs.registrar_conexao_do_canal(canal, false, t0);
    obs.registrar_conexao_do_canal(canal, true, t0 + segundos(5));
    obs.registrar_conexao_do_canal(canal, true, t0 + segundos(6));
    obs.registrar_conexao_do_canal(canal, false, t0 + segundos(100));
    obs.registrar_conexao_do_canal(canal, false, t0 + segundos(101));
    obs.registrar_conexao_do_canal(canal, true, t0 + segundos(130));

    let s = obs.snapshot(t0 + segundos(200));
    assert_eq!(s.channels.len(), 1);
    let c = &s.channels[0];
    assert_eq!(c.channel, canal);
    assert!(c.connected);
    assert_eq!(c.connections, 2, "o `true` repetido nao e outra conexao");
    assert_eq!(c.reconnects, 1);
    assert_eq!(c.drops, 1);
    assert_eq!(c.failed_attempts, 2);
    assert_eq!(c.last_change_ago_s, Some(70));
}

#[test]
fn tendencia_de_armazenamento_guarda_janela_limitada() {
    let obs = Observabilidade::new();
    let t0 = Instant::now();
    let n = MAX_AMOSTRAS as u64 + 10;
    for i in 0..n {
        obs.registrar_amostra(
            Armazenamento::MemoriaEntradas,
            100 + i,
            t0 + segundos(300 * i),
        );
    }
    obs.registrar_amostra(Armazenamento::RunsNoLedger, 500, t0);
    obs.registrar_amostra(Armazenamento::RunsNoLedger, 200, t0 + segundos(60));

    let agora = t0 + segundos(300 * (n - 1));
    let s = obs.snapshot(agora);
    let recursos: Vec<&str> = s.storage.iter().map(|x| x.resource).collect();
    assert_eq!(recursos, ["memory_entries", "agent_runs"]);
    let mem = &s.storage[0];
    assert_eq!(mem.samples.len(), MAX_AMOSTRAS);
    assert_eq!(mem.current, Some(100 + n - 1));
    assert_eq!(mem.samples[0].value, 110, "as mais antigas saem");
    assert_eq!(mem.delta, Some(MAX_AMOSTRAS as i64 - 1));
    assert_eq!(mem.window_s, 300 * (MAX_AMOSTRAS as u64 - 1));
    assert_eq!(mem.samples.last().map(|a| a.ago_s), Some(0));
    let runs = &s.storage[1];
    assert_eq!(runs.current, Some(200));
    assert_eq!(
        runs.delta,
        Some(-300),
        "a retencao apagou: tendencia negativa"
    );
}

#[test]
fn registro_vazio_tem_snapshot_vazio() {
    let s = Observabilidade::new().snapshot(Instant::now());
    assert!(s.tools.is_empty() && s.mcp.is_empty() && s.channels.is_empty());
    assert!(s.storage.is_empty());
    assert_eq!(s.tools_total.calls, 0);
    assert_eq!(
        s.tools_total.outcomes.len(),
        Desfecho::TODOS.len(),
        "o total lista todo desfecho, mesmo sem chamada"
    );
    assert_eq!(s.tools_total.success_rate, None);
}

#[test]
fn lock_envenenado_nao_derruba_quem_registra() {
    let obs = Arc::new(Observabilidade::new());
    let t0 = Instant::now();
    obs.registrar(chamada("file_read", Desfecho::Sucesso, 1), t0);
    let clone = Arc::clone(&obs);
    let r = std::thread::spawn(move || {
        let _guard = clone.inner.lock().expect("lock");
        panic!("panico segurando o lock");
    })
    .join();
    assert!(r.is_err());
    assert!(obs.inner.is_poisoned());
    obs.registrar(chamada("file_read", Desfecho::Erro, 1), t0);
    let s = obs.snapshot(t0);
    assert_eq!(
        linha(&s, "file_read").stats.calls,
        2,
        "o que ja estava contado fica"
    );
}
