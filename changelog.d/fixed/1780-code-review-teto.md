- **`code_review` deixa de morrer no teto do modo (#1780).** A ferramenta
  chama um LLM por dentro (30-120s de rotina) e a chamada nao tinha timeout
  proprio: quem a matava era o wrap externo do runtime, no teto do modo —
  20s no `review` —, virando a revisao inteira em `tool timeout`. O LLM
  ganha teto proprio de 180s dentro da tool, com erro nomeando a etapa; a
  trait `Tool` ganha `teto_proprio`, e quem declara o bound total que
  respeita (diff + LLM, no caso) nao e morta pelo wrap externo antes dele —
  o teto do modo continua valendo para todas as outras ferramentas, e nada
  levanta teto. O modo `review` sobe de 20s para 120s. Timeout nao e
  controle de autorizacao: sem efeito em seguranca.
