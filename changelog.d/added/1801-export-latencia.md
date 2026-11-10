- **Latencia de `GET /v1/me/export` medida em cenario reproduzivel (#1801).**
  Novo teste de integracao `rest_v1_me_export_perf`: 10 mil mensagens da
  titular em dois grupos, pelo router autenticado contra PostgreSQL real, com
  o corpo consumido inteiro e conteudo/isolamento validados fora da janela
  medida. O teste afirma so a referencia historica de 30 s e reporta p50/p95;
  receita (`run.sh`), maquina, versoes, SHA e amostras cruas ficam em
  `benches/data-export/`. O cenario registra tambem o teto do endpoint: com
  10 mil mensagens o export traz 5 000 e declara o corte em
  `truncated_sections`.
