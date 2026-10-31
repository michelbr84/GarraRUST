- **`200` vazio entra no circuito de retry/fallback (#1776).** Um `Ok` sem
  texto e sem bloco de ferramenta em `complete_reportando_provider` saia do
  circuito como sucesso de primeira - sem retry no primario, sem fallback e
  sem registro no breaker; o turno so aparecia vazio no guarda do chamador,
  quando o circuito inteiro ja tinha sido pulado. Agora o 200 vazio recebe o
  mesmo tratamento de um erro reintentavel (breaker, backoff dentro do
  orcamento de retry, queda para os fallbacks e a ultima resposta vazia de
  volta no esgotamento - os guardas de turno existentes seguem valendo), e o
  consumo das tentativas descartadas vai embutido no `usage` final. Turno
  so-de-ferramenta continua sendo sucesso.
