- **Rede caida nao acionava o fallback para o provider local** (#1249). A
  classificacao de retry casava texto de erro (`429`, `5xx`, `upstream`) e
  falha de transporte (`connection refused`, DNS, timeout de conexao) nao casa
  com padrao nenhum — o turno morria em `Err(e) => return Err(e)` ANTES do
  laco de fallback, nos caminhos batch e streaming: usuario tira o cabo e
  recebe erro em vez do Ollama que ja roda na maquina (ADR 0022, local-first).
  Agora existe a variante tipada `Error::Transport`, construida no ponto onde
  o `reqwest::Error` ainda existe como tipo (`providers::erro_de_envio`,
  chamado nos quatro providers — openai, anthropic, ollama, llama_cpp — e nos
  dois bracos, batch e streaming de cada um); o runtime classifica pela CLASSE
  e nao pela frase (`is_transport_error`). Transporte gasta **uma** tentativa
  e cai direto para o fallback, sem queimar o orcamento de retry (insistir 4x
  num endereco inalcancavel queimava ~3,5s de backoff); o breaker recebe a
  falha do mesmo jeito. 429/5xx continua no comportamento historico (retry
  com backoff no mesmo endereco). Testes com tempo virtual (`test-util`)
  provam a contagem de tentativas nos dois caminhos e o turno completo pelo
  fallback local.
