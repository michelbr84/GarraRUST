//! #1425 (opcao B): ferramenta registrada mas **nao operacional** fica FORA
//! da lista chamavel do modelo e, se pedida mesmo assim, volta como resultado
//! de ferramenta com o motivo estruturado — nunca executa. O `garra_status` e
//! os diagnosticos e que a mostram como indisponivel (fora desta crate).

use super::*;
use crate::tools::Disponibilidade;
use std::sync::atomic::{AtomicBool, Ordering};

/// Uma acao de canal cujo canal esta fora do ar.
struct CanalForaDoAr {
    executou: Arc<AtomicBool>,
}

#[async_trait]
impl Tool for CanalForaDoAr {
    fn name(&self) -> &str {
        "canal_send"
    }
    fn description(&self) -> &str {
        "manda mensagem por um canal"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _c: &ToolContext, _i: serde_json::Value) -> Result<ToolOutput> {
        self.executou.store(true, Ordering::SeqCst);
        Ok(ToolOutput::success("mandei"))
    }
    fn disponibilidade(&self) -> Disponibilidade {
        Disponibilidade::indisponivel(
            "channel_offline",
            "o canal Telegram esta configurado, mas nao esta conectado agora.",
            Some(
                "Veja `garra_status`; o operador liga o canal com `garraia channel connect`."
                    .into(),
            ),
        )
    }
}

#[test]
fn disponibilidade_de_le_a_tool_registrada_e_nome_desconhecido_e_disponivel() {
    let rt = AgentRuntime::new();
    rt.register_tool(stub("file_read"));
    rt.register_tool(Box::new(CanalForaDoAr {
        executou: Arc::new(AtomicBool::new(false)),
    }));
    assert!(rt.disponibilidade_de("file_read").e_disponivel());
    assert!(rt.disponibilidade_de("nunca-registrada").e_disponivel());
    match rt.disponibilidade_de("canal_send") {
        Disponibilidade::Indisponivel {
            codigo,
            motivo,
            remediacao,
        } => {
            assert_eq!(codigo, "channel_offline");
            assert!(motivo.contains("nao esta conectado"));
            assert!(remediacao.is_some());
        }
        Disponibilidade::Disponivel => panic!("canal fora do ar deveria ser indisponivel"),
    }
    let texto = rt.disponibilidade_de("canal_send").explicacao("canal_send");
    assert!(
        texto.contains("indisponivel agora (channel_offline)"),
        "{texto}"
    );
    assert!(texto.contains("Nao repita"), "{texto}");
}

/// O modelo nao ve a ferramenta na lista do turno, e se a pedir pelo nome
/// mesmo assim, ela nao roda e a recusa volta como resultado de ferramenta.
#[tokio::test]
async fn ferramenta_indisponivel_fica_fora_da_lista_e_nao_executa_se_pedida() {
    let rt = AgentRuntime::new();
    let provider = Arc::new(mcp_no_runtime::PedeFerramentaMcp::novo("canal_send"));
    rt.register_provider(provider.clone());
    let executou = Arc::new(AtomicBool::new(false));
    rt.register_tool(Box::new(CanalForaDoAr {
        executou: Arc::clone(&executou),
    }));
    rt.register_tool(stub("file_read"));

    // Modo sem whitelist (`code`): o PORTAO deixaria `canal_send` passar; o
    // que a tira da lista e a disponibilidade.
    let exec = ExecContext::with_mode(Some("code".to_string()));
    let resposta = rt
        .process_message_with_agent_config(
            "sessao-1425",
            "avisa o time",
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
        .expect("o turno termina");

    let vistas = provider.ferramentas_da_primeira_volta();
    assert!(vistas.contains(&"file_read".to_string()), "{vistas:?}");
    assert!(
        !vistas.contains(&"canal_send".to_string()),
        "ferramenta indisponivel na lista chamavel: {vistas:?}"
    );
    assert!(
        !executou.load(Ordering::SeqCst),
        "a ferramenta indisponivel RODOU quando o modelo a pediu pelo nome"
    );
    let resultados = provider.resultados();
    assert!(
        resultados
            .iter()
            .any(|r| r.contains("indisponivel agora (channel_offline)")
                && r.contains("nao esta conectado")),
        "a recusa volta ao modelo como resultado de ferramenta, com codigo e motivo; veio {resultados:?}"
    );
    assert!(
        !resultados.iter().any(|r| r.contains("nao e permitida")),
        "indisponivel nao e o mesmo que negada pela politica: {resultados:?}"
    );
    assert_eq!(resposta, "segui sem ela");
}

/// Toda lista que o modelo ve e o despacho passam pelo mesmo par
/// (portao, disponibilidade). Guarda de fonte: o filtro das tres listas e uma
/// funcao so, e o despacho consulta a disponibilidade depois do portao.
#[test]
fn listas_e_despacho_consultam_a_disponibilidade() {
    let fonte = include_str!("../../runtime.rs");
    assert!(
        fonte.matches("self.definicoes_do_turno(&portao").count() >= 3,
        "as tres listas do modelo passam por `definicoes_do_turno`"
    );
    assert!(
        fonte.contains("let disponibilidade = self.disponibilidade_de(name);"),
        "o despacho consulta a disponibilidade da tool pedida"
    );
}
