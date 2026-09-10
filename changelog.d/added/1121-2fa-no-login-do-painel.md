- O login do painel admin passa a exigir o segundo fator quando a conta tem
  2FA ligado. Antes `POST /admin/api/login` parava na senha: o TOTP (RFC 6238)
  que o projeto ja implementava so era alcancavel pelo fluxo mobile
  (`/auth/2fa/*`), entao uma senha vazada abria o console inteiro. Com 2FA, a
  resposta de login sem codigo vem `401` com `totp_required: true` — nao
  `401` generico — porque o cliente precisa distinguir "falta o codigo" de
  "senha errada" para poder mostrar o campo em vez de repetir a senha (#1121).
- Quatro rotas de enrollment, todas dentro do router autenticado, atras de
  sessao e CSRF: `GET /admin/api/2fa/status`, `POST /admin/api/2fa/setup`
  (gera e guarda um segredo **pendente**), `POST /admin/api/2fa/verify`
  (primeiro codigo valido liga), `POST /admin/api/2fa/disable` (exige o codigo
  atual — desligar o segundo fator e exatamente o que um invasor com a senha
  tentaria, entao nao pode ser so uma confirmacao de sessao). Um segredo
  pendente nao vale nada: so passa a ser cobrado depois que o `verify` roda,
  o que evita travar o dono fora do painel por um setup abandonado (#1121).
- Seis codigos errados em 15 minutos travam o segundo fator por usuario, com
  `429`; um acerto zera a contagem. Sem isso o TOTP de 6 digitos seria
  forcavel por forca bruta a partir da propria resposta de erro. Cada
  terminal — setup, verify, disable, e cada recusa no login — escreve no
  `audit_log` com o IP (#1121).
- Girar o segredo com o 2FA ligado e recusado com `409`: substituir o segredo
  por baixo do dono deixaria o app dele apontando para o antigo, sem que nada
  tivesse pedido confirmacao. O caminho e desligar com um codigo valido e
  refazer o setup (#1121).
- A pagina **Security** do console ganha o enrollment: mostra o segredo base32
  e a URI `otpauth://` como texto copiavel. Nenhum QR e gerado por biblioteca
  ou servico de imagem — isso mandaria o segredo para um terceiro (#1121).
