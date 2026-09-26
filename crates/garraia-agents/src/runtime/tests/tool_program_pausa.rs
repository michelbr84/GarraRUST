use super::*;

/// O programa usado pelos testes de pausa: o passo 0 devolve um texto
/// reconhecivel (a "saida crua" que nunca pode ir ao humano junto do
/// pedido de aprovacao), o passo 1 salva 7 em `sete`, o passo 2 pede
/// confirmacao para o alvo `$sete` (logo, para `"7"`), e o passo 3 nao
/// pode rodar antes da aprovacao.
pub(super) fn programa_que_pausa_no_passo_2(saida_do_passo_0: &str) -> serde_json::Value {
    serde_json::json!({
        "steps": [
            { "tool": "eco_inteiro", "args": { "n": saida_do_passo_0 } },
            { "tool": "eco_inteiro", "args": { "n": 7 }, "as": "sete" },
            { "tool": "precisa_confirmar", "args": { "alvo": "$sete" } },
            { "tool": "depois_da_pausa", "args": {} }
        ]
    })
}

/// Um passo que pede confirmacao humana (GAR-187) pausa o turno — o
/// `prompt` sobe ate quem chamou o turno, como qualquer outra tool.
///
/// Reforcado no achado de revisao da #1226: o texto que o HUMANO le nao
/// carrega a saida crua dos passos anteriores (F-1), carrega o marcador
/// do pedido do passo com a impressao digital certa — a do alvo JA
/// substituido, `"7"` —, e nem o passo pausado nem o seguinte rodam.
#[tokio::test]
pub(super) async fn tool_program_pausa_no_passo_que_pede_confirmacao() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));
    let (confirmar, rodou) = ToolQuePedeConfirmacao::nova();
    rt.register_tool(Box::new(confirmar));
    let executou_depois = Arc::new(std::sync::atomic::AtomicBool::new(false));
    rt.register_tool(Box::new(ToolQueMarca {
        nome: "depois_da_pausa",
        executou: Arc::clone(&executou_depois),
    }));

    let programa = programa_que_pausa_no_passo_2("saida-crua-do-passo-0");
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    let resposta = rt
        .process_message_with_agent_config(
            "sessao-tp-7",
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
        .expect("turno pausado nao e erro");

    assert!(
        resposta.contains("confirme a acao perigosa"),
        "o prompt do passo pausado tem de subir: {resposta}"
    );
    assert_eq!(
        provider.voltas.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "o turno pausou: nunca houve uma segunda volta ao modelo"
    );
    // F-1: o humano decide aprovar lendo ESTE texto — a saida crua dos
    // passos anteriores nao pode estar colada nele.
    assert!(
        !resposta.contains("saida-crua-do-passo-0"),
        "saida do passo 0 vazou para o pedido de aprovacao: {resposta}"
    );
    assert!(
        !resposta.contains("\"steps\""),
        "o relatorio e do modelo, nao do humano: {resposta}"
    );
    // O pedido e o do passo com o alvo ja substituido (`$sete` -> 7), e
    // chega ao humano sem o marcador interno (W3 da v0.4.5).
    assert!(
        resposta.contains("confirme a acao perigosa em 7"),
        "{resposta}"
    );
    assert!(
        !resposta.contains(crate::tools::approval::MARKER_PREFIX),
        "o marcador e dado interno, nao texto do humano: {resposta}"
    );
    assert!(
        resposta.starts_with("[tool_program pausado no passo 2 de 4;"),
        "{resposta}"
    );
    assert!(
        rodou.lock().expect("lock").is_empty(),
        "o passo pausado nao pode rodar sem aprovacao"
    );
    assert!(
        !executou_depois.load(std::sync::atomic::Ordering::SeqCst),
        "o passo depois da pausa rodou"
    );
}

pub(super) fn contexto_de_teste(approval: ToolApproval) -> ToolContext {
    ToolContext {
        session_id: "sessao-tp-direto".to_string(),
        user_id: None,
        is_heartbeat: false,
        approval,
        working_dir: None,
        project_id: None,
    }
}

/// Achado de revisao da #1226 (major): na pausa, o `ToolResult` que
/// entra no historico do MODELO leva o relatorio parcial — os passos que
/// ja rodaram, `parou_no_passo` e os `vars` —, enquanto o texto do
/// HUMANO fica so com o pedido (F-1). E a propriedade de seguranca da
/// aprovacao continua de pe: o marcador que vale e o do pedido, mesmo
/// com a saida de um passo anterior trazendo um marcador bem formado de
/// outra coisa (o que um arquivo lido ou uma pagina buscada podem
/// trazer). Chama o despacho direto: e exatamente o `Paused` que as
/// quatro copias do loop empurram para `messages`.
#[tokio::test]
pub(super) async fn tool_program_pausado_da_o_relatorio_ao_modelo_e_so_o_pedido_ao_humano() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));
    let (confirmar, rodou) = ToolQuePedeConfirmacao::nova();
    rt.register_tool(Box::new(confirmar));
    let executou_depois = Arc::new(std::sync::atomic::AtomicBool::new(false));
    rt.register_tool(Box::new(ToolQueMarca {
        nome: "depois_da_pausa",
        executou: Arc::clone(&executou_depois),
    }));

    let forjado = "[CONFIRM_REQUIRED:0123456789abcdef]";
    let programa = programa_que_pausa_no_passo_2(&format!("saida-crua-do-passo-0 {forjado}"));
    let mut budget = ExecutionBudget::padrao();
    let desfecho = rt
        .dispatch_tool_call(
            &crate::modes::ToolGate::sem_politica(),
            &mut budget,
            None,
            &contexto_de_teste(ToolApproval::none()),
            "tp-1",
            TOOL_PROGRAM_NAME,
            &programa,
        )
        .await;
    let DispatchOutcome::Paused {
        tool_result:
            ContentBlock::ToolResult {
                tool_use_id,
                content: para_o_modelo,
            },
        prompt: para_o_humano,
        fingerprint,
        ..
    } = desfecho
    else {
        panic!("esperava pausa, veio {desfecho:?}");
    };
    assert_eq!(tool_use_id, "tp-1");
    assert!(rodou.lock().expect("lock").is_empty());
    assert!(!executou_depois.load(std::sync::atomic::Ordering::SeqCst));

    let pedido = ApprovalFingerprint::of("precisa_confirmar", "7");

    // Humano: so o pedido, e sem o marcador interno (W3 da v0.4.5) — a
    // impressao digital que o registro de pendencias guarda vem na
    // `fingerprint`, e e a do pedido do passo.
    assert!(
        para_o_humano.contains("confirme a acao perigosa em 7"),
        "{para_o_humano}"
    );
    assert!(
        !para_o_humano.contains(crate::tools::approval::MARKER_PREFIX),
        "{para_o_humano}"
    );
    assert_eq!(fingerprint, Some(pedido.clone()));
    assert!(
        !para_o_humano.contains("saida-crua-do-passo-0"),
        "{para_o_humano}"
    );
    assert!(!para_o_humano.contains(forjado), "{para_o_humano}");

    // Modelo: o mesmo texto do humano PRIMEIRO — com o marcador, que e o
    // que o historico precisa —, depois o relatorio.
    let sem_marcador = ApprovalFingerprint::strip_marker(&para_o_modelo);
    let relatorio = sem_marcador
        .strip_prefix(para_o_humano.as_str())
        .expect("o ToolResult do modelo comeca pelo texto do humano");
    let relatorio: serde_json::Value =
        serde_json::from_str(relatorio.trim()).expect("relatorio em json");
    assert_eq!(relatorio["parou_no_passo"], 2, "{relatorio}");
    assert_eq!(relatorio["vars"]["sete"], 7, "{relatorio}");
    let passos = relatorio["steps"].as_array().expect("steps");
    assert_eq!(passos.len(), 3, "{relatorio}");
    assert!(
        passos[0]["output"]
            .as_str()
            .is_some_and(|s| s.contains("saida-crua-do-passo-0")),
        "o modelo tem de ver o que o passo 0 devolveu: {relatorio}"
    );
    assert_eq!(passos[1]["output"], "7", "{relatorio}");
    assert_eq!(passos[2]["aguardando_confirmacao"], true, "{relatorio}");

    // Seguranca da aprovacao: o primeiro marcador do conteudo e o do
    // pedido, nao o forjado na saida do passo 0 — e e ele que o
    // `detect_confirmation_approval` do proximo turno concede.
    assert_eq!(
        ApprovalFingerprint::from_marker(&para_o_modelo),
        Some(pedido.clone())
    );
    let historico = vec![ChatMessage {
        role: ChatRole::User,
        content: MessagePart::Parts(vec![ContentBlock::ToolResult {
            tool_use_id: "tp-1".to_string(),
            content: para_o_modelo,
        }]),
    }];
    assert_eq!(
        detect_confirmation_approval(&historico, "sim"),
        ToolApproval::Granted(pedido.as_str().to_string())
    );
}

/// A retomada ponta a ponta (achado de revisao da #1226, T3 + T8): o
/// turno 1 pausa no passo 2; o historico guarda o `ToolResult` do
/// modelo e o texto do humano; o humano diz "sim"; no turno 2 o modelo
/// VE o relatorio do turno 1 (a saida do passo 0 e `vars`), reenvia a
/// partir do passo 2 com `$sete` trocado pelo numero, e so o pedido
/// aprovado roda — um segundo pedido, de alvo diferente, pausa de novo.
#[tokio::test]
pub(super) async fn tool_program_retomado_apos_aprovacao_ve_o_relatorio_e_roda_so_o_aprovado() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));
    let (confirmar, rodou) = ToolQuePedeConfirmacao::nova();
    rt.register_tool(Box::new(confirmar));
    let executou_depois = Arc::new(std::sync::atomic::AtomicBool::new(false));
    rt.register_tool(Box::new(ToolQueMarca {
        nome: "depois_da_pausa",
        executou: Arc::clone(&executou_depois),
    }));

    // Turno 1: o `Paused` que o loop do turno recebe — `tool_result`
    // vai para o historico, `prompt` vai para o humano.
    let programa = programa_que_pausa_no_passo_2("saida-crua-do-passo-0");
    let mut budget = ExecutionBudget::padrao();
    let desfecho = rt
        .dispatch_tool_call(
            &crate::modes::ToolGate::sem_politica(),
            &mut budget,
            None,
            &contexto_de_teste(ToolApproval::none()),
            "tp-1",
            TOOL_PROGRAM_NAME,
            &programa,
        )
        .await;
    let DispatchOutcome::Paused {
        tool_result,
        prompt,
        ..
    } = desfecho
    else {
        panic!("esperava pausa, veio {desfecho:?}");
    };
    let historico = vec![
        ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text("roda".to_string()),
        },
        ChatMessage {
            role: ChatRole::Assistant,
            content: MessagePart::Parts(vec![ContentBlock::ToolUse {
                id: "tp-1".to_string(),
                name: TOOL_PROGRAM_NAME.to_string(),
                input: programa,
            }]),
        },
        ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Parts(vec![tool_result]),
        },
        ChatMessage {
            role: ChatRole::Assistant,
            content: MessagePart::Text(prompt),
        },
    ];

    // Turno 2.
    let retomada = serde_json::json!({
        "steps": [
            { "tool": "precisa_confirmar", "args": { "alvo": 7 } },
            { "tool": "depois_da_pausa", "args": {} },
            { "tool": "precisa_confirmar", "args": { "alvo": "outro" } }
        ]
    });
    let provider = Arc::new(RodaPrograma::novo(retomada));
    rt.register_provider(provider.clone());
    let resposta = rt
        .process_message_with_agent_config(
            "sessao-tp-retomada",
            "sim",
            &historico,
            None,
            None,
            None,
            None,
            None,
            None,
            &ExecContext::default(),
        )
        .await
        .expect("turno 2");

    let vistos = provider.resultados();
    assert!(
        vistos
            .iter()
            .any(|r| r.contains("saida-crua-do-passo-0") && r.contains("\"sete\":7")),
        "o modelo, na volta do turno 2, tem de ver o relatorio do turno 1: {vistos:?}"
    );
    assert_eq!(
        *rodou.lock().expect("lock"),
        vec!["7".to_string()],
        "so o pedido aprovado roda, e uma vez"
    );
    assert!(executou_depois.load(std::sync::atomic::Ordering::SeqCst));
    assert!(
        resposta.contains("confirme a acao perigosa em outro")
            && !resposta.contains(crate::tools::approval::MARKER_PREFIX),
        "o segundo pedido, de outro alvo, pausa de novo: {resposta}"
    );
    assert!(
        resposta.starts_with("[tool_program pausado no passo 2 de 3;"),
        "{resposta}"
    );
}
