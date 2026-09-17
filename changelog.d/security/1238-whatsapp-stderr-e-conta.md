- **A cauda do stderr do Node deixa de sair verbatim no terminal (#1238).** Ela
  e a unica saida crua de ferramenta externa do fluxo de vinculo do WhatsApp, e
  nada a redigia: o lado JS redige JID e digitos, mas nao base64 de credencial,
  e um `throw` vindo de dentro da biblioteca nem passa por la. Agora sequencias
  longas que parecem material cifrado viram `<redigido: N caracteres>` e a
  linha tem teto de comprimento, sem perder o que a cauda existe para mostrar
  (caminho de arquivo, nome de modulo, numero de linha).
- **A varredura que impede log de sessao passou a ser uma regra fechada
  (#1238).** Ela reprovava uma lista de macros conhecidas; um `eprintln!` ou um
  `tracing::event!` com o valor da sessao passavam, e nenhuma varredura de
  macro alcancaria `let s = blob.expose(); let t = s;`. A regra foi invertida:
  toda ocorrencia de `SessionBlob::expose()` no codigo de producao do modulo
  precisa estar numa allowlist de call sites nomeados — hoje ha exatamente
  dois. O parser tambem deixou de ficar cego quando o fonte tem um literal de
  char com aspa dentro (`b'"'`), que invertia todas as fronteiras de literal
  e fazia o arquivo inteiro render zero bloco, em silencio.
- **A conta da sessao recusa nomes de dispositivo do Windows (#1238).**
  `con`, `prn`, `aux`, `nul`, `com1`-`com9` e `lpt1`-`lpt9` casavam a allowlist
  `[A-Za-z0-9_-]` e nao podem virar diretorio no Windows. A recusa e
  case-insensitive e da um erro que diz o que houve, em vez de um
  `ERROR_INVALID_NAME` opaco no meio da criacao do diretorio.
