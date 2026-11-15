use super::super::{ToolSource, pode_receber_snapshot_de_ferramentas};

#[test]
fn snapshot_so_e_entregue_a_tools_nativas_de_introspeccao() {
    let nativa = ToolSource::Native;
    let mcp = ToolSource::Mcp {
        server: "servidor-teste".into(),
    };

    assert!(pode_receber_snapshot_de_ferramentas(
        "garra_status",
        &nativa
    ));
    assert!(pode_receber_snapshot_de_ferramentas("cli_inspect", &nativa));
    assert!(!pode_receber_snapshot_de_ferramentas("cli_inspect", &mcp));
    assert!(!pode_receber_snapshot_de_ferramentas(
        "tool_mcp_qualquer",
        &mcp
    ));
    assert!(!pode_receber_snapshot_de_ferramentas(
        "tool_nativa_qualquer",
        &nativa
    ));
}
