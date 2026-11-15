use super::super::{
    CLI_INSPECT_TOOL_NAME, GARRA_DIAGNOSTICS_TOOL_NAME, GARRA_STATUS_TOOL_NAME, ToolSource,
    pode_receber_snapshot_de_ferramentas,
};

#[test]
fn snapshot_so_e_entregue_a_tools_nativas_de_introspeccao() {
    let nativa = ToolSource::Native;
    let mcp = ToolSource::Mcp {
        server: "servidor-teste".into(),
    };

    for name in [
        GARRA_STATUS_TOOL_NAME,
        GARRA_DIAGNOSTICS_TOOL_NAME,
        CLI_INSPECT_TOOL_NAME,
    ] {
        assert!(pode_receber_snapshot_de_ferramentas(name, &nativa));
        assert!(!pode_receber_snapshot_de_ferramentas(name, &mcp));
    }
    assert!(!pode_receber_snapshot_de_ferramentas(
        "tool_mcp_qualquer",
        &mcp
    ));
    assert!(!pode_receber_snapshot_de_ferramentas(
        "tool_nativa_qualquer",
        &nativa
    ));
}
