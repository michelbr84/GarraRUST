- **O tamanho da memoria aparece no `/metrics` (#957, fecha a issue).** O #994
  entregou quatro metricas de **instrumentacao** — elas contam o que aconteceu
  quando aconteceu. Faltavam as de **estado**: quantas entradas existem agora e
  quantas estao no indice vetorial. Estado nao tem evento, entao alguem precisa
  ir olhar. Entram `garraia_memory_entries{has_embedding}` e
  `garraia_memory_vector_index_size`.
- **Os dois devem ser lidos juntos, e a distancia entre eles e o sinal.** Uma
  entrada com vetor na coluna mas fora do indice nao aparece na busca
  semantica. Isso era invisivel ate alguem rodar `garra memory stats` — que so
  mostra quando perguntam. Como gauge, vira tendencia, que e o que faz alguem
  pensar em perguntar. A consulta esta no `docs/telemetry.md`.
- **Worker proprio, e nao um braco do de retencao.** Era o caminho obvio, ja
  que existe um laco periodico tocando a memoria, e e errado por dois motivos
  independentes: o `memory_retention_worker` so sobe quando
  `memory.retention.enabled` e true, que nasce false porque apaga dado — os
  gauges ficariam mortos em quase toda instalacao; e a cadencia dele e de 24h,
  que nao mostra tendencia, mostra dois pontos por semana. Verificado no
  binario: com a retencao desligada (o padrao), o log diz que aquele worker nao
  subiu e os gauges estao servindo assim mesmo.
- **Leitura barata, e nao o relatorio de integridade.** O `integrity_report()`
  que alimenta o `garra memory stats` faz um `SELECT id FROM memory_entries`
  inteiro mais varredura de orfaos — trabalho justificado sob demanda,
  desperdicio a cada poucos minutos. O `gauge_snapshot()` novo sao tres
  `count(*)`, com as **mesmas** consultas, e ha teste cobrando que os dois
  concordem: dois numeros que deviam ser iguais e nao sao e o pior caso para
  quem esta diagnosticando.
- Falha de leitura nao derruba o laco: o proximo tick tenta de novo. Um gauge
  que para de atualizar em silencio e pior que um que some, porque o Prometheus
  continua servindo o ultimo valor e o painel mostra numero velho como atual.
- Achados da auditoria tratados antes do merge: a leitura solta o mutex do banco
  principal **antes** de consultar o vetorial (segurar os dois nao trava hoje,
  porque nenhum caminho adquire na ordem inversa, mas bloqueava recall e
  remember durante o tick e deixava a armadilha armada para o proximo caminho
  com locking invertido); contagem inconsistente (`com vetor > total`, que so
  acontece com corrupcao) passa a gritar em vez de publicar zero mudo; e entra
  `garraia_memory_gauge_errors_total`, porque um gauge que congela em silencio e
  pior que um que some — o Prometheus continua servindo o ultimo valor como se
  fosse atual.
