- **O painel de capacidades do console admin mostra a camada que barrou (#1915).**
  O registro ja entregava `denied_by` e `denied_because` em cada linha negada
  (`GET /admin/api/capabilities?session_id=`), mas a tabela de Sessions >
  Capabilities so exibia o motivo em texto. Entrou a coluna `Denied by`, com a
  camada (modo da sessao, teto do principal ou os dois) e o motivo dentro dela;
  a linha carrega `data-denied-by` / `data-denied-because` para os testes. Um
  valor que o console nao conhece sai cru, sem traducao inventada. So
  exibicao: nenhum modo, teto, raiz ou jail foi alterado.
