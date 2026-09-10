- O painel admin ganha recuperacao de senha sem e-mail: `garra admin recovery
  start --username X` gera um codigo de uso unico que nao volta na resposta
  HTTP. O gateway guarda so o hash PBKDF2 do codigo e escreve o texto num
  arquivo `0600` no diretorio de dados, entao ler o codigo exige shell na
  maquina — a mesma barra de quem roda o CLI. `garra admin recovery complete`
  consome o codigo uma unica vez, troca a senha (minimo 8 caracteres, igual ao
  resto do admin) e revoga as sessoes antigas (#1122).
- `POST /admin/api/recovery/start` responde sempre o mesmo corpo, existindo o
  usuario ou nao, e gasta o mesmo trabalho de PBKDF2 nos dois casos para nao
  virar um oraculo de enumeracao de usuarios. Com o painel ainda sem nenhum
  usuario a rota se recusa a gerar codigo: nada a recuperar, e ela nao pode
  virar caminho de criacao de conta (#1122).
- As duas rotas de recuperacao sao as primeiras do admin com limitacao por IP
  (10/min, o `RateLimiter` de auth do gateway). Cada inicio invalida o codigo
  anterior pendente do mesmo usuario, e se o arquivo nao pode ser escrito o
  codigo e descartado — nao fica vivo um codigo que ninguem consegue ler
  (#1122).
- A tela de login do admin mostra um "Esqueci minha senha" que explica o fluxo
  e os dois comandos do CLI. Nao ha formulario de envio nem mencao a e-mail: o
  canal do codigo e o host (#1122).
