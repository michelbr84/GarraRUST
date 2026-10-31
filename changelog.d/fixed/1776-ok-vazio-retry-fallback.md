- **`200` vazio entra no circuito de retry/fallback (#1776).** Um `Ok` sem
  texto e sem bloco de ferramenta em `complete_reportando_provider` saia do
  circuito como sucesso de primeira - sem retry no primario, sem fallback e
  sem registro no breaker; o turno so aparecia vazio no guarda do chamador,
  quando o circuito inteiro ja tinha sido pulado. Agora o 200 vazio recebe o
  mesmo tratamento de um erro reintentavel: uma re-tentativa no primario, e o
  segundo vazio seguido vai direto para os fallbacks sem queimar o orcamento
  de retry inteiro (mesmo criterio do #1249 para transporte); o breaker do
  primario recebe UMA falha por turno de vazias, nao uma por tentativa; no
  esgotamento, a ultima resposta vazia volta e os guardas de turno existentes
  seguem valendo. O gasto faturado das tentativas descartadas nao se perde:
  sai em campo proprio (`RespostaDoCircuito.uso_descartado`), enquanto o
  `usage` da resposta devolvida segue intocado - o valor de origem de quem
  serviu, que e o que o shim `POST /v1/messages` repassa como medidor de
  janela de contexto do cliente. Turno so-de-ferramenta continua sendo
  sucesso.
