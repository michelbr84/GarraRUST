- **ADR 0020 propoe a crate `garraia-hardware` (epic #1124).** Documenta a
  decisao arquitetural que a regra absoluta 8 exige antes do primeiro commit
  de codigo do epic: `trait Device` + `Capability` com risk class R0-R5
  embutida desde o inicio (issues #1125+#1129), adapters (MQTT, Home
  Assistant, Serial/GPIO) aditivos ao core, reusando `ToolApproval`/
  `safety_gate` em vez de duplicar o gate de risco. Status `Proposed` de
  proposito: nenhum codigo nasce ate o dono aceitar.
