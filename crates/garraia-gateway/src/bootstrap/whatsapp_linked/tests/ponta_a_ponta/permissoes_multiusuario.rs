//! #1427: o modelo multi-principal (ADR 0025) provado de ponta a ponta —
//! ponte falsa → sink → portao → runtime → provider → despacho de
//! ferramenta —, com a config VIVA, como em producao.
//!
//! Cada cenario varia so o que a politica diz que decide: o principal
//! (dono, usuario `read`, usuario `read+write`, desconhecido em `open`,
//! desconhecido em `restricted`, dono dentro de grupo, bloqueado), o perfil
//! de execucao e o piso. O provider e um stub que PEDE a ferramenta pelo
//! nome, sem olhar a lista — e o que se observa e se ela rodou (espiao MCP e
//! o `ok` da nativa no `tool_result`) ou se voltou a recusa estruturada.
//!
//! Nenhuma linha de producao muda aqui: sao provas do que #1489/#1494/#1497
//! entregaram, na fiacao inteira.

use super::*;

const ESTRANHO: &str = "5531944443333";
const NATIVA_ESCRITA: &str = "file_write";
const NATIVA_LEITURA: &str = "file_read";
/// A recusa estruturada do portao — pelo TETO do principal ("pela politica de
/// acesso") ou pelo PISO do modo ("no modo `search`"); as duas sao recusas
/// que voltam ao modelo como `tool_result`, e nenhuma executa nada.
const RECUSA_DA_POLITICA: &str = "nao e permitida";

/// O espiao de escrita MCP COM classe de capacidade: o `McpTool` real deriva
/// `McpWrite` do nome da operacao (`write_file`); o `ToolEspia` generico do
/// harness nao tem classe nenhuma e, sob um teto por classe, e negado
/// fail-closed — o que aqui confundiria "teto aplicado" com "espiao sem
/// classe". Este espiao declara a classe que a ferramenta real teria.
struct EspiaMcpDeEscrita {
    chamadas: Contador,
}

#[async_trait::async_trait]
impl garraia_agents::tools::Tool for EspiaMcpDeEscrita {
    fn name(&self) -> &str {
        ESCRITA_MCP
    }
    fn description(&self) -> &str {
        "espiao mcp de escrita"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    fn capacidades(&self) -> &'static [garraia_agents::capacidades::Capacidade] {
        &[garraia_agents::capacidades::Capacidade::McpWrite]
    }
    async fn execute(
        &self,
        _context: &garraia_agents::tools::ToolContext,
        _input: serde_json::Value,
    ) -> garraia_common::Result<garraia_agents::tools::ToolOutput> {
        self.chamadas
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(garraia_agents::tools::ToolOutput {
            content: "escrito".to_string(),
            is_error: false,
            requires_confirmation: false,
        })
    }
}

fn registra_espia_mcp(chamadas: &Contador) -> impl FnOnce(&SharedState) {
    let chamadas = Arc::clone(chamadas);
    move |state: &SharedState| {
        state
            .agents
            .replace_mcp_tools("filesystem", vec![Box::new(EspiaMcpDeEscrita { chamadas })]);
    }
}

/// A secao viva com politica v2. `default_mode: code` sobe o PISO para
/// todos, e assim o que separa `read` de `read+write` e de dono e o TETO do
/// principal — exatamente o que estes testes querem ver.
fn viva_v2(access: serde_json::Value, piso_code: bool, grupos: bool) -> AppConfig {
    let mut secao_json = serde_json::json!({
        "allow": [],
        "owners": [],
        "access": access,
        "reply_in_groups": grupos,
    });
    if piso_code {
        secao_json["default_mode"] = serde_json::json!("code");
    }
    config_com(Some(secao(Some(true), secao_json)))
}

/// Uma mensagem 1:1 de outro numero (nao o `PEER`).
fn msg_de(remetente: &str, texto: &str) -> InboundMessage {
    let mut m = msg(Some(texto));
    m.chat_jid = Jid::new(format!("{remetente}@s.whatsapp.net"));
    m.sender_jid = Jid::new(format!("{remetente}@s.whatsapp.net"));
    m.sender_phone = Some(format!("+{remetente}"));
    m
}

/// Sobe com config viva, provider proprio e o gancho de preparo (espiao MCP).
///
/// `default_mode` e `reply_in_groups` sao knobs de BOOT: `admissao_vigente`
/// rele por turno `enabled`/`allow`/`owners`/`access`, e so eles. Por isso
/// os dois entram aqui, na montagem, e nao na config viva.
async fn sobe_v2(
    provider: ProviderDeStub,
    perfil: ExecutionProfile,
    rx: watch::Receiver<AppConfig>,
    piso_code: bool,
    grupos: bool,
    preparo: impl FnOnce(&SharedState),
) -> Cenario {
    let c = Montagem {
        roteiro: Roteiro::eco().da_propria_conta(),
        perfil,
        provider,
        config_viva: Some(rx),
        reply_in_groups: grupos,
        default_mode: piso_code.then(|| "code".to_string()),
        ..Montagem::default()
    }
    .sobe(preparo)
    .await;
    assert!(
        ate(|| c.state.whatsapp_linked.bridge() == BridgeView::Connected).await,
        "a ponte precisa estar de pe"
    );
    c
}

/// Entrega uma mensagem qualquer pelo sink real e espera o espiao registra-la.
async fn entrega_msg(c: &Cenario, m: InboundMessage) {
    let antes = recebidas(c).len();
    InboundSink::deliver(&*c.espiao, m);
    assert!(
        ate(|| recebidas(c).len() > antes).await,
        "a mensagem precisa chegar ao sink"
    );
}

fn ferramentas_do_turno(c: &Cenario, i: usize) -> Vec<String> {
    turnos(&c.provider)
        .get(i)
        .map(|t| t.ferramentas.clone())
        .unwrap_or_default()
}

/// O que o modelo viu na SEGUNDA rodada do turno `i` (a que carrega o
/// `tool_result`): a recusa estruturada, ou a saida da ferramenta.
fn retorno_da_ferramenta(c: &Cenario, rodada: usize) -> String {
    turnos(&c.provider)
        .get(rodada)
        .map(|t| t.texto_do_usuario.clone())
        .unwrap_or_default()
}

/// **Usuario `read`**: le o workspace aprovado, e nao muda nada — nem pela
/// ferramenta nativa nem pela MCP; os dois caminhos dao a MESMA recusa.
/// Depois, a politica muda a quente para `write: true` e a mensagem
/// SEGUINTE ja escreve pelos dois caminhos. Por fim, `blocked: true` a
/// quente: a mensagem seguinte nem vira turno, e a recusa fica contada.
#[tokio::test]
async fn usuario_read_le_mas_nao_muda_e_o_write_a_quente_vale_no_turno_seguinte() {
    let read = serde_json::json!({ "users": { PEER: { "level": "read" } } });
    let (tx, rx) = watch::channel(viva_v2(read, true, false));
    let chamadas = contador();
    let c = sobe_v2(
        pedido_de_escrita(),
        ExecutionProfile::Standard,
        rx,
        true,
        false,
        registra_espia_mcp(&chamadas),
    )
    .await;

    // Turno 1: pede a escrita MCP. Lista sem escrita, espiao parado, recusa
    // estruturada de volta ao modelo.
    entrega(&c, "escreve pela mcp").await;
    assert_eq!(
        turnos_estaveis_em(&c, 2).await,
        2,
        "{:?}",
        turnos(&c.provider)
    );
    let vistas = ferramentas_do_turno(&c, 0);
    assert!(
        vistas.iter().any(|f| f == NATIVA_LEITURA),
        "leitura e oferecida: {vistas:?}"
    );
    for escondida in [NATIVA_ESCRITA, ESCRITA_MCP, "bash"] {
        assert!(
            !vistas.iter().any(|f| f == escondida),
            "`{escondida}` nao pode ser oferecida a um usuario read: {vistas:?}"
        );
    }
    assert_eq!(rodou(&chamadas), 0, "a escrita MCP nao roda para read");
    assert!(
        retorno_da_ferramenta(&c, 1).contains(RECUSA_DA_POLITICA),
        "o modelo recebe a recusa estruturada: {}",
        retorno_da_ferramenta(&c, 1)
    );

    // Turno 2: pede a escrita NATIVA. Mesma recusa, mesmo texto.
    c.provider.rearma(
        NATIVA_ESCRITA,
        serde_json::json!({ "path": "n.txt", "content": "x" }),
    );
    entrega(&c, "escreve nativo").await;
    assert_eq!(turnos_estaveis_em(&c, 4).await, 4);
    let nativa = retorno_da_ferramenta(&c, 3);
    assert!(
        nativa.contains(RECUSA_DA_POLITICA) && !nativa.contains("\\\"ok\\\""),
        "nativa e MCP dao o mesmo resultado para read: {nativa}"
    );

    // Turno 3: pede a LEITURA nativa — roda (`ok` da fixture).
    c.provider
        .rearma(NATIVA_LEITURA, serde_json::json!({ "path": "n.txt" }));
    entrega(&c, "le").await;
    assert_eq!(turnos_estaveis_em(&c, 6).await, 6);
    assert!(
        retorno_da_ferramenta(&c, 5).contains("ok"),
        "read le o workspace: {}",
        retorno_da_ferramenta(&c, 5)
    );

    // Politica a quente: write on. A mensagem seguinte escreve pelos dois.
    let read_write = serde_json::json!({ "users": { PEER: { "level": "read", "write": true } } });
    tx.send(viva_v2(read_write, true, false)).expect("watcher");
    c.provider.rearma(
        ESCRITA_MCP,
        serde_json::json!({ "path": "n.txt", "content": "y" }),
    );
    entrega(&c, "agora escreve pela mcp").await;
    assert_eq!(turnos_estaveis_em(&c, 8).await, 8);
    assert_eq!(
        rodou(&chamadas),
        1,
        "write a quente libera a escrita MCP no turno seguinte"
    );
    assert!(
        ferramentas_do_turno(&c, 6)
            .iter()
            .any(|f| f == NATIVA_ESCRITA),
        "e a nativa passa a ser oferecida: {:?}",
        ferramentas_do_turno(&c, 6)
    );
    c.provider.rearma(
        NATIVA_ESCRITA,
        serde_json::json!({ "path": "n.txt", "content": "z" }),
    );
    entrega(&c, "agora escreve nativo").await;
    assert_eq!(turnos_estaveis_em(&c, 10).await, 10);
    assert!(
        retorno_da_ferramenta(&c, 9).contains("ok"),
        "nativa e MCP dao o mesmo resultado para read+write: {}",
        retorno_da_ferramenta(&c, 9)
    );
    for vistas in [ferramentas_do_turno(&c, 6), ferramentas_do_turno(&c, 8)] {
        assert!(
            !vistas.iter().any(|f| f == "bash"),
            "write e SO escrita de arquivo, nunca bash: {vistas:?}"
        );
    }

    // Bloqueio a quente: a mensagem seguinte nem vira turno.
    let bloqueado = serde_json::json!({
        "users": { PEER: { "level": "read", "write": true, "blocked": true } }
    });
    tx.send(viva_v2(bloqueado, true, false)).expect("watcher");
    entrega(&c, "ainda estou aqui?").await;
    assert_eq!(
        turnos_estaveis_em(&c, 11).await,
        10,
        "bloqueado nao vira turno"
    );
    assert_eq!(
        c.state
            .whatsapp_linked
            .rejeicoes()
            .de(rejeicoes::Motivo::Bloqueado),
        1,
        "e a recusa fica contada como blocked_user"
    );

    encerra(c).await;
}

/// **Desconhecido**: em `restricted` nao entra (sem turno, recusa contada);
/// em `open` entra como CHAT — nenhuma ferramenta oferecida, e um pedido
/// pelo nome volta como recusa da politica, mesmo com o piso em `code`.
#[tokio::test]
async fn desconhecido_e_negado_em_restricted_e_so_conversa_em_open() {
    let restrita = serde_json::json!({ "admission": "restricted" });
    let (tx, rx) = watch::channel(viva_v2(restrita, true, false));
    let c = sobe_v2(
        ProviderDeStub::que_pede(NATIVA_LEITURA, serde_json::json!({ "path": "n.txt" })),
        ExecutionProfile::Standard,
        rx,
        true,
        false,
        |_| {},
    )
    .await;

    entrega_msg(&c, msg_de(ESTRANHO, "oi")).await;
    assert_eq!(turnos_estaveis_em(&c, 1).await, 0, "restricted: sem turno");
    assert_eq!(
        c.state
            .whatsapp_linked
            .rejeicoes()
            .de(rejeicoes::Motivo::Restrita),
        1
    );

    let aberta = serde_json::json!({ "admission": "open", "default": { "level": "chat" } });
    tx.send(viva_v2(aberta, true, false)).expect("watcher");
    entrega_msg(&c, msg_de(ESTRANHO, "oi de novo")).await;
    assert_eq!(
        turnos_estaveis_em(&c, 2).await,
        2,
        "open: entra, e vira turno"
    );
    assert!(
        ferramentas_do_turno(&c, 0).is_empty(),
        "chat: nenhuma ferramenta oferecida, mesmo com piso code: {:?}",
        ferramentas_do_turno(&c, 0)
    );
    assert!(
        retorno_da_ferramenta(&c, 1).contains(RECUSA_DA_POLITICA),
        "pedido pelo nome volta como recusa: {}",
        retorno_da_ferramenta(&c, 1)
    );

    encerra(c).await;
}

/// **Dono**: `role: owner` na politica v2 nao confere poder por si — em
/// `standard` o dono fica no piso `search` (sem `bash`, sem escrita); so o
/// perfil `isolated-pod` da o piso `code` ao dono em conversa 1:1.
#[tokio::test]
async fn dono_recebe_full_so_quando_o_perfil_de_execucao_permite() {
    let dono = serde_json::json!({ "users": { PEER: { "role": "owner" } } });
    for (perfil, espera_bash) in [
        (ExecutionProfile::Standard, false),
        (ExecutionProfile::IsolatedPod, true),
    ] {
        let (_tx, rx) = watch::channel(viva_v2(dono.clone(), false, false));
        let c = sobe_v2(ProviderDeStub::default(), perfil, rx, false, false, |_| {}).await;
        entrega(&c, "oi").await;
        assert_eq!(turnos_estaveis_em(&c, 1).await, 1, "{perfil:?}");
        let vistas = ferramentas_do_turno(&c, 0);
        assert_eq!(
            vistas.iter().any(|f| f == "bash"),
            espera_bash,
            "{perfil:?}: dono com bash so no pod: {vistas:?}"
        );
        assert_eq!(
            vistas.iter().any(|f| f == NATIVA_ESCRITA),
            espera_bash,
            "{perfil:?}: dono escreve so no pod: {vistas:?}"
        );
        encerra(c).await;
    }
}

/// **Grupo nao herda o Full do dono**: no pod, com grupos ligados na
/// politica (`default: read`), a mensagem do dono DENTRO do grupo roda como
/// grupo — leitura oferecida, nada de `bash` nem escrita, e o pedido de
/// escrita MCP nao roda.
#[tokio::test]
async fn grupo_nao_herda_o_full_do_dono() {
    let dono_e_grupos = serde_json::json!({
        "users": { PEER: { "role": "owner" } },
        "groups": { "enabled": true, "default": { "level": "read" } }
    });
    let (_tx, rx) = watch::channel(viva_v2(dono_e_grupos, false, true));
    let chamadas = contador();
    let c = sobe_v2(
        pedido_de_escrita(),
        ExecutionProfile::IsolatedPod,
        rx,
        false,
        true,
        registra_espia_mcp(&chamadas),
    )
    .await;

    entrega_msg(&c, no_grupo(PEER, "escreve ai")).await;
    assert_eq!(
        turnos_estaveis_em(&c, 2).await,
        2,
        "{:?}",
        turnos(&c.provider)
    );
    let vistas = ferramentas_do_turno(&c, 0);
    assert!(
        vistas.iter().any(|f| f == NATIVA_LEITURA),
        "grupo read le: {vistas:?}"
    );
    for escondida in ["bash", NATIVA_ESCRITA, ESCRITA_MCP] {
        assert!(
            !vistas.iter().any(|f| f == escondida),
            "`{escondida}` nao chega ao grupo, mesmo com o dono dentro: {vistas:?}"
        );
    }
    assert_eq!(rodou(&chamadas), 0, "a escrita MCP nao roda no grupo");
    assert!(
        retorno_da_ferramenta(&c, 1).contains(RECUSA_DA_POLITICA),
        "{}",
        retorno_da_ferramenta(&c, 1)
    );

    // Controle: o MESMO dono, 1:1, no mesmo pod, escreve.
    c.provider.rearma(
        ESCRITA_MCP,
        serde_json::json!({ "path": "n.txt", "content": "1:1" }),
    );
    entrega(&c, "escreve no privado").await;
    assert_eq!(turnos_estaveis_em(&c, 4).await, 4);
    assert_eq!(rodou(&chamadas), 1, "controle: 1:1 no pod o dono escreve");

    encerra(c).await;
}
