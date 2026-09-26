use super::*;

/// Criterio de aceite da issue (S-B, texto literal): "programa nao
/// alcanca tool que o loop normal negaria no mesmo `ExecContext`
/// (table-driven sobre todos os perfis)". Para cada modo nativo,
/// compara o portao DIRETO (`ToolGate::from_profile`, sem passar por
/// `tool_program`) contra o que o `tool_program` de fato conseguiu
/// executar — os dois tem de bater, nos dois sentidos: nem o programa
/// alcanca o que o modo negaria, nem o modo bloqueia o que ele
/// permitiria.
///
/// Duas sondas por modo (achado de revisao da #1226): a que nenhuma
/// lista nomeia — so exercita whitelist, e nos modos sem whitelist
/// (`auto`, `code`, `ask`) sempre espera `true` — e, para CADA nome do
/// `denied` do perfil, uma sonda com aquele nome, que tem de dar
/// `false`. Sem a segunda, um gate por passo que honrasse so a
/// whitelist passaria na tabela inteira.
#[tokio::test]
pub(super) async fn tool_program_bate_com_o_gate_direto_em_todos_os_perfis_nativos() {
    let mut negadas_exercitadas = 0usize;
    for modo in crate::modes::AgentMode::all_modes() {
        // Achado de revisao (provado por mutacao): sem isto, nos
        // perfis whitelist o `tool_program` e barrado no TOPO
        // (`permite("tool_program")` ja da false) e o gate POR PASSO
        // nunca chega a ser exercido — o teste passaria ate sem ele.
        // Liberar `tool_program` aqui forca o programa a entrar no
        // loop de passos em todo modo, e e so ai que a propriedade da
        // issue ("nao alcanca o que o modo negaria") fica provada.
        let mut perfil = crate::modes::ModeProfile::from_mode(modo);
        perfil
            .tool_policy
            .allowed
            .push(TOOL_PROGRAM_NAME.to_string());
        let portao = crate::modes::ToolGate::from_profile(&perfil);

        let mut sondas = vec!["sonda-nunca-em-allowlist-nenhuma".to_string()];
        sondas.extend(perfil.tool_policy.denied.iter().cloned());

        for sonda in sondas {
            let esperado = portao.permite(&sonda);
            if perfil.tool_policy.denied.contains(&sonda) {
                assert!(!esperado, "modo {modo:?}: `denied` tem de vencer: {sonda}");
                negadas_exercitadas += 1;
            }

            let rt = AgentRuntime::new();
            let executou = Arc::new(std::sync::atomic::AtomicBool::new(false));
            rt.register_tool(Box::new(SondaComNome {
                nome: sonda.clone(),
                executou: Arc::clone(&executou),
            }));
            let exec = ExecContext {
                custom_profile: Some(perfil.clone()),
                ..Default::default()
            };
            let programa = serde_json::json!({
                "steps": [ { "tool": sonda, "args": {} } ]
            });
            let provider = Arc::new(RodaPrograma::novo(programa));
            rt.register_provider(provider.clone());

            rt.process_message_with_agent_config(
                &format!("sessao-modo-{}-{sonda}", modo.as_str()),
                "roda",
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                &exec,
            )
            .await
            .expect("turno");

            assert_eq!(
                executou.load(std::sync::atomic::Ordering::SeqCst),
                esperado,
                "modo {modo:?}, sonda `{sonda}`: tool_program executou={} mas o \
                 gate direto permite={esperado}",
                executou.load(std::sync::atomic::Ordering::SeqCst),
            );
            if !esperado {
                // A recusa tem de vir do gate POR PASSO (o programa
                // entrou no loop), e nao do topo.
                let resultados = provider.resultados();
                let corpo: serde_json::Value = serde_json::from_str(&resultados[0]).expect("json");
                assert_eq!(corpo["parou_no_passo"], 0, "{modo:?}/{sonda}: {corpo}");
                assert!(
                    corpo["steps"][0]["denied"]
                        .as_str()
                        .is_some_and(|s| s.contains("nao e permitida no modo")),
                    "{modo:?}/{sonda}: {corpo}"
                );
            }
        }
    }
    assert!(
        negadas_exercitadas > 0,
        "nenhum perfil nativo tem `denied` — a tabela nao exercitaria a lista"
    );
}

/// Achado de revisao da #1226 (issue: "`bash` ou `device_execute` sob
/// modo read-only"): `ask` e o modo nativo que EXPOE `tool_program` (sem
/// whitelist) e ao mesmo tempo nega ferramentas. Com o perfil NATIVO,
/// sem nenhuma alteracao, e montado pelo nome como o gateway monta
/// (`agent_mode: "ask"`), um programa nao alcanca nada do `denied`.
#[tokio::test]
pub(super) async fn tool_program_no_modo_ask_nativo_nao_alcanca_o_que_ask_nega() {
    let perfil = crate::modes::ModeProfile::from_mode(crate::modes::AgentMode::Ask);
    let portao = crate::modes::ToolGate::from_profile(&perfil);
    assert!(
        portao.permite(TOOL_PROGRAM_NAME),
        "premissa: `ask` nativo expoe tool_program — senao a recusa viria do topo"
    );
    for obrigatoria in ["bash", "device_execute", "file_write"] {
        assert!(
            perfil.tool_policy.denied.iter().any(|d| d == obrigatoria),
            "premissa: `ask` nega {obrigatoria}"
        );
    }

    for negada in perfil.tool_policy.denied.clone() {
        let rt = AgentRuntime::new();
        let executou = Arc::new(std::sync::atomic::AtomicBool::new(false));
        rt.register_tool(Box::new(SondaComNome {
            nome: negada.clone(),
            executou: Arc::clone(&executou),
        }));
        let exec = ExecContext {
            agent_mode: Some("ask".to_string()),
            ..Default::default()
        };
        let programa = serde_json::json!({
            "steps": [ { "tool": negada, "args": { "command": "echo furou" } } ]
        });
        let provider = Arc::new(RodaPrograma::novo(programa));
        rt.register_provider(provider.clone());

        rt.process_message_with_agent_config(
            &format!("sessao-ask-{negada}"),
            "roda",
            &[],
            None,
            None,
            None,
            None,
            None,
            None,
            &exec,
        )
        .await
        .expect("turno");

        assert!(
            !executou.load(std::sync::atomic::Ordering::SeqCst),
            "`{negada}` rodou dentro de um tool_program no modo ask"
        );
        let resultados = provider.resultados();
        let corpo: serde_json::Value = serde_json::from_str(&resultados[0]).expect("json");
        assert_eq!(corpo["parou_no_passo"], 0, "{negada}: {corpo}");
        assert!(
            corpo["steps"][0]["denied"]
                .as_str()
                .is_some_and(|s| s.contains("nao e permitida no modo")),
            "{negada}: {corpo}"
        );
    }
}

/// Achado de revisao da #1226: QUAIS perfis nativos, sem nenhuma
/// alteracao, expoem `tool_program` hoje. Os sem whitelist (`auto`,
/// `code`, `ask`) expoem — o gate de cada passo continua valendo —; os
/// com whitelist nao o listam e nao expoem. Mudar isto tem de ser
/// decisao deliberada (S-C), e este teste e quem a torna visivel.
#[test]
pub(super) fn tool_program_exposto_so_nos_perfis_nativos_sem_whitelist() {
    use crate::modes::AgentMode;
    let esperado =
        |modo: AgentMode| matches!(modo, AgentMode::Auto | AgentMode::Code | AgentMode::Ask);

    let rt = AgentRuntime::new();
    rt.register_tool(stub("file_read"));
    let todas = rt.tool_definitions();

    let modos = AgentMode::all_modes();
    assert_eq!(
        modos.len(),
        9,
        "modo nativo novo: decida se expoe tool_program"
    );
    for modo in modos {
        let portao =
            crate::modes::ToolGate::from_profile(&crate::modes::ModeProfile::from_mode(modo));
        // O mesmo filtro que o turno aplica na montagem da lista.
        let visivel = todas
            .iter()
            .filter(|d| portao.permite(&d.name))
            .any(|d| d.name == TOOL_PROGRAM_NAME);
        assert_eq!(
            portao.permite(TOOL_PROGRAM_NAME),
            esperado(modo),
            "modo {modo:?}: exposicao de tool_program mudou"
        );
        assert_eq!(visivel, esperado(modo), "modo {modo:?}: lista do modelo");
    }

    // Sessao sem modo escolhido: portao aberto, expoe (o gate por passo
    // nao tem politica a aplicar, como nas chamadas diretas).
    assert!(crate::modes::ToolGate::sem_politica().permite(TOOL_PROGRAM_NAME));
}

/// Orcamento de passos do programa: mais que `MAX_PROGRAM_STEPS` e
/// erro do `tool_program`, nao do turno.
#[tokio::test]
pub(super) async fn tool_program_recusa_mais_passos_que_o_orcamento() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));
    let passos: Vec<_> = (0..(MAX_PROGRAM_STEPS + 1))
        .map(|i| serde_json::json!({ "tool": "eco_inteiro", "args": { "n": i } }))
        .collect();
    let programa = serde_json::json!({ "steps": passos });
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    rt.process_message_with_agent_config(
        "sessao-tp-5",
        "roda",
        &[],
        None,
        None,
        None,
        None,
        None,
        None,
        &ExecContext::default(),
    )
    .await
    .expect("turno: programa mal formado nao aborta");

    let resultados = provider.resultados();
    assert!(
        resultados[0].contains("excede o orcamento"),
        "{resultados:?}"
    );
}

/// Orcamento **por passo** dentro do proprio programa: o turno inteiro
/// tem teto de 10 chamadas (#979, o modo nunca levanta), e um
/// `tool_program` de 16 passos nao pode contornar isso so porque nao
/// volta ao modelo entre passos.
#[tokio::test]
pub(super) async fn tool_program_respeita_o_orcamento_de_chamadas_do_turno() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));
    let passos: Vec<_> = (0..MAX_PROGRAM_STEPS)
        .map(|i| serde_json::json!({ "tool": "eco_inteiro", "args": { "n": i } }))
        .collect();
    let programa = serde_json::json!({ "steps": passos });
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    // Achado de revisao (bloqueador 2): estourar so o teto do TURNO
    // (10, com a tarefa — 50 — ainda com folga) nao pode abortar a
    // conversa. O loop principal, no mesmo caso, so reseta o contador;
    // `tool_program` para graciosamente e devolve o relatorio parcial,
    // e o loop principal segue no proximo turno (aqui, a segunda volta
    // do provider, que encerra com texto).
    let resposta = rt
        .process_message_with_agent_config(
            "sessao-tp-6",
            "roda",
            &[],
            None,
            None,
            None,
            None,
            None,
            None,
            &ExecContext::default(),
        )
        .await
        .expect("orcamento de TURNO (com folga na tarefa) nao aborta a conversa");

    assert_eq!(resposta, "concluido");
    let resultados = provider.resultados();
    let corpo: serde_json::Value = serde_json::from_str(&resultados[0]).expect("json");
    let passos_executados = corpo["steps"].as_array().expect("steps");
    assert_eq!(
        passos_executados.len(),
        9,
        "1 (tool_program) + 9 passos = 10 = max_per_turn; o 10o passo para: {corpo}"
    );
    assert!(
        corpo["motivo"]
            .as_str()
            .is_some_and(|m| m.contains("orcamento do turno")),
        "{corpo}"
    );
}

/// O outro lado do achado acima: quando e a TAREFA que esgota (nao so
/// o turno), o `tool_program` aborta a conversa — mesma classe de erro
/// que o loop principal ja usa nesse caso. Perfil customizado com
/// `max_tool_loops = 5` faz `max_per_turn == max_per_task == 5`
/// (`com_limites_do_modo`), entao os dois esgotam juntos e
/// `atingiu_limite_turno` (que exige folga na tarefa) fica falso.
#[tokio::test]
pub(super) async fn tool_program_aborta_a_conversa_quando_a_tarefa_esgota() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));

    let mut perfil = crate::modes::ModeProfile::from_mode(crate::modes::AgentMode::Code);
    perfil.limits = crate::modes::ModeLimits {
        max_tool_loops: 5,
        timeout_secs: 30,
        max_turns: 10,
    };
    let exec = ExecContext {
        custom_profile: Some(perfil),
        ..Default::default()
    };

    let passos: Vec<_> = (0..6)
        .map(|i| serde_json::json!({ "tool": "eco_inteiro", "args": { "n": i } }))
        .collect();
    let programa = serde_json::json!({ "steps": passos });
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    let erro = rt
        .process_message_with_agent_config(
            "sessao-tp-6b",
            "roda",
            &[],
            None,
            None,
            None,
            None,
            None,
            None,
            &exec,
        )
        .await
        .expect_err("tarefa (nao so turno) esgotada tem de abortar a conversa");

    let msg = erro.to_string();
    assert!(msg.contains("execution budget exceeded"), "{msg}");
    assert!(msg.contains("tool_program"), "{msg}");
}
