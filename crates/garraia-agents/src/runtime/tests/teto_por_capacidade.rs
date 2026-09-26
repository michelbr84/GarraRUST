use super::*;

/// #1385: o runtime pergunta ao portao com as classes da ferramenta
/// registrada — e assim que um teto por classe vale sobre `file_write`
/// (nativa) e `filesystem__write_file` (MCP) sem entrada por nome.
#[test]
fn portao_permite_consulta_as_classes_da_tool_registrada() {
    use crate::modes::{Nivel, TetoDeCapacidades, ToolGate, politica_do_nivel};
    let rt = AgentRuntime::new();
    rt.register_tool(stub("file_write"));
    rt.register_tool(stub("file_read"));
    rt.replace_mcp_tools("filesystem", vec![stub("filesystem__write_file")]);
    let teto = TetoDeCapacidades {
        nome: "acesso read do WhatsApp".into(),
        politica: politica_do_nivel(Nivel::Read, false),
    };
    let portao = ToolGate::for_mode_name("code").com_teto(Some(teto));
    assert!(rt.portao_permite(&portao, "file_read"));
    assert!(
        !rt.portao_permite(&portao, "file_write"),
        "teto read barra a escrita nativa"
    );
    // O stub MCP nao tem anotacao, mas `write_file` e operacao de filesystem
    // por nome — fica sem classe aqui porque o stub e nativo de mentira; o
    // que se afirma e o fail-closed: sem classe, o teto por classe nao libera.
    assert!(!rt.portao_permite(&portao, "filesystem__write_file"));
    assert!(
        !rt.portao_permite(&portao, "inexistente"),
        "nome desconhecido e fail-closed sob teto por classe"
    );
}

/// **A fiacao.** Todo ponto do runtime que filtra ou despacha pergunta por
/// `portao_permite` (nome + classe); o `portao.permite(` por nome so sobrevive
/// nos avisos, que falam da lista antes do filtro.
#[test]
fn todo_filtro_e_despacho_do_runtime_passa_por_portao_permite() {
    let fonte = include_str!("../../runtime.rs");
    let por_nome = fonte.matches("portao.permite(").count();
    let por_classe = fonte.matches("self.portao_permite(").count();
    assert!(
        por_classe >= 5,
        "esperava >= 5 pontos por classe (3 listas + despacho + garra_status), achei {por_classe}"
    );
    assert!(
        por_nome <= 1,
        "sobrou `portao.permite(` por nome fora dos avisos: {por_nome}"
    );
}
