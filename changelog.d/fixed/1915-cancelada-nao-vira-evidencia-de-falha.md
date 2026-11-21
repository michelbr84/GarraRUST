- **Chamada cancelada nao vira mais evidencia de falha (#1915).** Quando o
  turno caia com uma ferramenta em andamento, o `ToolFinished` com
  `outcome: cancelled` era projetado por `evidence_from_event` como
  `EvidenceRef { success: false }`, e `detect_divergencia` acusava de
  contradicao uma alegacao de sucesso que o ledger nao tinha como desmentir.
  Agora a chamada cancelada nao gera evidencia: o resultado fica
  desconhecido, como ja acontecia na projecao da CLI (`nao_verificado`).
  Nenhum modo, teto, raiz ou jail mudou.
