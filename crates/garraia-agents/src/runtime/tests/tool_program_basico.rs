use super::*;

/// O modelo ve `tool_program` na lista de tools, e o schema exige
/// `steps`.
#[test]
pub(super) fn tool_program_aparece_nas_definicoes_do_runtime() {
    let rt = AgentRuntime::new();
    rt.register_tool(stub("bash"));
    let defs = rt.tool_definitions();
    let def = defs
        .iter()
        .find(|d| d.name == TOOL_PROGRAM_NAME)
        .expect("tool_program tem de estar nas definicoes, ao lado de uma tool real");
    assert_eq!(def.input_schema["required"], serde_json::json!(["steps"]));
}

/// Achado de revisao (F-5 / importante 7): runtime sem nenhuma tool
/// registrada nao ganha `tool_program` de graca — nao ha nada pra um
/// programa executar, e o `tool_count == 0` de
/// `apply_tools_model_override` precisa continuar valendo 0.
#[test]
pub(super) fn tool_program_nao_aparece_sem_nenhuma_tool_registrada() {
    let rt = AgentRuntime::new();
    assert!(rt.tool_definitions().is_empty());
}

/// Criterio de aceite da #1226 S-B: um pedido multi-tool resolve num
/// unico turno, passo 2 recebe o inteiro que o passo 1 guardou via `as`
/// e `$meio` — substituido como NUMERO, nunca como texto.
#[tokio::test]
pub(super) async fn tool_program_executa_passos_em_sequencia_com_substituicao_de_var() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));

    let programa = serde_json::json!({
        "steps": [
            { "tool": "eco_inteiro", "args": { "n": 41 }, "as": "meio" },
            { "tool": "eco_inteiro", "args": { "n": "$meio" } }
        ]
    });
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    let resposta = rt
        .process_message_with_agent_config(
            "sessao-tp-1",
            "roda o programa",
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
        .expect("o turno tem de terminar");

    assert_eq!(resposta, "concluido");
    let resultados = provider.resultados();
    assert_eq!(
        resultados.len(),
        1,
        "um so ToolResult: o do tool_program agregado, nao um por passo"
    );
    let corpo: serde_json::Value = serde_json::from_str(&resultados[0]).expect("json");
    let passos = corpo["steps"].as_array().expect("steps");
    assert_eq!(passos.len(), 2);
    assert_eq!(passos[0]["output"], "41");
    assert_eq!(
        passos[1]["output"], "41",
        "passo 2 recebeu o inteiro do passo 1 via $meio, nao o texto \"$meio\": {corpo}"
    );
}

/// "so valor inteiro": saida que nao parseia como inteiro nao vira
/// variavel — e, desde o achado de revisao da #1226, o passo que
/// declarou `as` FALHA ali, com `parou_no_passo` apontando para ele.
/// Antes o passo saia `"ok": true` e o seguinte recebia o literal
/// `"$v"` (fail-open: num `bash`, `$v` seria expandido como variavel de
/// ambiente). O passo seguinte nao roda.
#[tokio::test]
pub(super) async fn tool_program_nao_substitui_saida_que_nao_e_inteiro_e_falha_o_passo() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));
    let executou_depois = Arc::new(std::sync::atomic::AtomicBool::new(false));
    rt.register_tool(Box::new(ToolQueMarca {
        nome: "depois_do_as",
        executou: Arc::clone(&executou_depois),
    }));

    let programa = serde_json::json!({
        "steps": [
            { "tool": "eco_inteiro", "args": { "n": "texto" }, "as": "v" },
            { "tool": "depois_do_as", "args": { "n": "$v" } }
        ]
    });
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    rt.process_message_with_agent_config(
        "sessao-tp-2",
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
    .expect("turno");

    let resultados = provider.resultados();
    let corpo: serde_json::Value = serde_json::from_str(&resultados[0]).expect("json");
    assert!(
        !executou_depois.load(std::sync::atomic::Ordering::SeqCst),
        "o passo seguinte rodou com o literal \"$v\": {corpo}"
    );
    let passos = corpo["steps"].as_array().expect("steps");
    assert_eq!(passos.len(), 1, "so o passo do `as` entra: {corpo}");
    assert_eq!(passos[0]["ok"], false, "{corpo}");
    assert_eq!(
        passos[0]["output"], "\"texto\"",
        "o passo rodou; a saida entra no relatorio: {corpo}"
    );
    assert!(
        passos[0]["erro"]
            .as_str()
            .is_some_and(|e| e.contains("inteiro") && e.contains("as: v")),
        "{corpo}"
    );
    assert_eq!(corpo["parou_no_passo"], 0, "{corpo}");
}

/// Achado de revisao da #1226 (fail-open): `"$nome"` sem valor falha o
/// passo ANTES do despacho, com `parou_no_passo` e o nome da variavel —
/// nunca chega a ferramenta como literal. Tres formas: nome com cara de
/// identificador nunca declarado (o caso da retomada apos pausa, em que
/// o programa reenviado nao traz os `vars` dos passos anteriores),
/// referencia adiantada a um nome declarado mais tarde (mesmo sem cara
/// de identificador), e referencia aninhada num array/objeto.
#[tokio::test]
pub(super) async fn tool_program_falha_o_passo_com_variavel_sem_valor_antes_de_rodar() {
    let casos = [
        (
            "nunca declarada",
            serde_json::json!({ "steps": [
                { "tool": "eco_inteiro", "args": { "n": 1 } },
                { "tool": "alvo", "args": { "command": "$n" } }
            ]}),
            1usize,
            "$n",
        ),
        (
            "declarada so depois",
            serde_json::json!({ "steps": [
                { "tool": "alvo", "args": { "v": "$total linhas" } },
                { "tool": "eco_inteiro", "args": { "n": 3 }, "as": "total linhas" }
            ]}),
            0usize,
            "$total linhas",
        ),
        (
            "aninhada",
            serde_json::json!({ "steps": [
                { "tool": "alvo", "args": { "lista": [ { "v": "$x" } ] } }
            ]}),
            0usize,
            "$x",
        ),
    ];

    for (caso, programa, parou, variavel) in casos {
        let rt = AgentRuntime::new();
        rt.register_tool(Box::new(EcoInteiroTool));
        let executou = Arc::new(std::sync::atomic::AtomicBool::new(false));
        rt.register_tool(Box::new(ToolQueMarca {
            nome: "alvo",
            executou: Arc::clone(&executou),
        }));
        let provider = Arc::new(RodaPrograma::novo(programa));
        rt.register_provider(provider.clone());

        rt.process_message_with_agent_config(
            &format!("sessao-tp-var-{caso}"),
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
        .expect("variavel sem valor e erro de passo, nao do turno");

        let resultados = provider.resultados();
        let corpo: serde_json::Value = serde_json::from_str(&resultados[0]).expect("json");
        assert!(
            !executou.load(std::sync::atomic::Ordering::SeqCst),
            "{caso}: a ferramenta rodou com o literal {variavel}: {corpo}"
        );
        assert_eq!(corpo["parou_no_passo"], parou, "{caso}: {corpo}");
        let passo = &corpo["steps"][parou];
        assert_eq!(passo["ok"], false, "{caso}: {corpo}");
        assert_eq!(
            passo["erro"],
            format!("variavel {variavel} nao definida"),
            "{caso}: {corpo}"
        );
        assert!(
            passo.get("output").is_none(),
            "{caso}: o passo nao rodou, nao ha saida a ecoar: {corpo}"
        );
    }
}

/// O outro lado da regra acima: string com `$` que NAO e referencia
/// (`"$HOME/bin/x"`, `"$"`, `"$ 5"`) segue intacta, como sempre seguiu
/// — o modelo poderia manda-la direto, e so `$` + nome e sintaxe do
/// programa.
#[tokio::test]
pub(super) async fn tool_program_texto_com_cifrao_que_nao_e_referencia_segue_intacto() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));
    let programa = serde_json::json!({
        "steps": [
            { "tool": "eco_inteiro", "args": { "n": "$HOME/bin/x" } },
            { "tool": "eco_inteiro", "args": { "n": "$" } },
            { "tool": "eco_inteiro", "args": { "n": "$ 5" } }
        ]
    });
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    rt.process_message_with_agent_config(
        "sessao-tp-cifrao",
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
    .expect("turno");

    let resultados = provider.resultados();
    let corpo: serde_json::Value = serde_json::from_str(&resultados[0]).expect("json");
    assert_eq!(corpo["steps"][0]["output"], "\"$HOME/bin/x\"", "{corpo}");
    assert_eq!(corpo["steps"][1]["output"], "\"$\"", "{corpo}");
    assert_eq!(corpo["steps"][2]["output"], "\"$ 5\"", "{corpo}");
    assert!(corpo.get("parou_no_passo").is_none(), "{corpo}");
}

/// Nucleo da #1226 S-B: um passo negado pelo gate do modo encerra o
/// programa, e os passos seguintes **nao rodam** — o mesmo portao do
/// loop normal, so que por passo.
#[tokio::test]
pub(super) async fn tool_program_para_no_passo_negado_pelo_modo_e_nao_roda_o_resto() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));
    let executou_negada = Arc::new(std::sync::atomic::AtomicBool::new(false));
    rt.register_tool(Box::new(ToolQueMarca {
        nome: "negada",
        executou: Arc::clone(&executou_negada),
    }));

    let perfil = crate::modes::ModeProfile::from_custom(
        crate::modes::AgentMode::Search,
        "so-eco",
        None,
        &serde_json::json!({ "allow": ["tool_program", "eco_inteiro"] }),
        &serde_json::json!({}),
    );
    let exec = ExecContext {
        custom_profile: Some(perfil),
        ..Default::default()
    };

    let programa = serde_json::json!({
        "steps": [
            { "tool": "eco_inteiro", "args": { "n": 1 } },
            { "tool": "negada", "args": {} },
            { "tool": "eco_inteiro", "args": { "n": 2 } }
        ]
    });
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    rt.process_message_with_agent_config(
        "sessao-tp-3",
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
        !executou_negada.load(std::sync::atomic::Ordering::SeqCst),
        "ferramenta fora da whitelist rodou dentro do tool_program"
    );
    let resultados = provider.resultados();
    let corpo: serde_json::Value = serde_json::from_str(&resultados[0]).expect("json");
    let passos = corpo["steps"].as_array().expect("steps");
    assert_eq!(
        passos.len(),
        2,
        "passo 0 (sucesso) e passo 1 (a recusa) entram no relatorio; o \
         passo 2 nao roda e nao aparece: {corpo}"
    );
    assert_eq!(passos[0]["ok"], true);
    assert_eq!(passos[1]["ok"], false);
    assert!(
        passos[1]["denied"]
            .as_str()
            .is_some_and(|s| s.contains("nao e permitida no modo")),
        "{corpo}"
    );
    assert_eq!(corpo["parou_no_passo"], 1);
}

/// `tool_program` nao pode chamar `tool_program` — aninhamento recusado
/// no passo, sem tentar um segundo despacho recursivo.
#[tokio::test]
pub(super) async fn tool_program_recusa_chamar_a_si_mesmo() {
    let rt = AgentRuntime::new();
    let programa = serde_json::json!({
        "steps": [ { "tool": "tool_program", "args": { "steps": [] } } ]
    });
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    rt.process_message_with_agent_config(
        "sessao-tp-4",
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
    .expect("turno");

    let resultados = provider.resultados();
    assert!(
        resultados[0].contains("aninhamento recusado"),
        "{resultados:?}"
    );
}

/// Achado de revisao (bloqueador 1): um passo que FALHA (tool nao
/// registrada, timeout, erro da propria tool) tem de encerrar o
/// programa como `"ok": false`, nao seguir como se tivesse dado certo.
/// Sem o fix, o passo 1 (que depende do resultado do passo 0) rodava
/// sobre uma dependencia quebrada.
#[tokio::test]
pub(super) async fn tool_program_para_no_passo_que_falha_e_nao_finge_sucesso() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));

    let programa = serde_json::json!({
        "steps": [
            { "tool": "tool-nao-registrada", "args": {} },
            { "tool": "eco_inteiro", "args": { "n": 5 } }
        ]
    });
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    rt.process_message_with_agent_config(
        "sessao-tp-erro",
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
    .expect("turno");

    let resultados = provider.resultados();
    let corpo: serde_json::Value = serde_json::from_str(&resultados[0]).expect("json");
    let passos = corpo["steps"].as_array().expect("steps");
    assert_eq!(
        passos.len(),
        1,
        "o passo 1 nao pode rodar depois de um passo 0 que falhou: {corpo}"
    );
    assert_eq!(passos[0]["ok"], false, "{corpo}");
    assert_eq!(corpo["parou_no_passo"], 0);
}

/// A deteccao de loop por assinatura (JANELA_LOOP=3) vale DENTRO de um
/// `tool_program` como vale no loop normal. Desde o #1295 item 1 a
/// primeira deteccao da tarefa avisa em vez de abortar: o terceiro passo
/// identico NAO roda, o programa para nele com o motivo rotulado como
/// loop (e nao como gate), e o modelo recebe a observacao corretiva. A
/// repeticao seguinte aborta — ver
/// `tool_program_repetido_entre_voltas_cai_no_detector_de_loop`.
#[tokio::test]
pub(super) async fn tool_program_avisa_no_loop_de_passos_identicos() {
    let rt = AgentRuntime::new();
    rt.register_tool(Box::new(EcoInteiroTool));

    let passo = serde_json::json!({ "tool": "eco_inteiro", "args": { "n": 1 } });
    let programa = serde_json::json!({ "steps": [passo.clone(), passo.clone(), passo] });
    let provider = Arc::new(RodaPrograma::novo(programa));
    rt.register_provider(provider.clone());

    let resposta = rt
        .process_message_with_agent_config(
            "sessao-tp-loop",
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
        .expect("o primeiro loop da tarefa avisa, nao aborta");
    assert_eq!(resposta, "concluido");

    let resultados = provider.resultados();
    let relatorio = resultados
        .iter()
        .find(|r| r.contains("\"steps\""))
        .expect("o modelo recebeu o relatorio do programa");
    let corpo: serde_json::Value = serde_json::from_str(relatorio).expect("relatorio e JSON");
    assert_eq!(
        corpo["motivo"], "loop detectado: aviso corretivo",
        "{corpo}"
    );
    assert_eq!(corpo["parou_no_passo"], 2, "{corpo}");
    let passo = &corpo["steps"][2];
    assert_eq!(passo["ok"], false, "{corpo}");
    let aviso = passo["loop"].as_str().expect("aviso do loop");
    assert!(aviso.contains("tool loop detected: eco_inteiro"), "{aviso}");
    assert!(aviso.contains("NAO foi executada"), "{aviso}");
    assert!(passo.get("denied").is_none(), "loop nao e gate: {corpo}");
}
