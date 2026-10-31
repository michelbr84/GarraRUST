- **Erro 3xx de provider e de `web_fetch` cita o destino do redirecionamento
  (#1779).** Os clientes de provider tem redirects desligados por seguranca,
  entao qualquer 3xx vira erro — mas a mensagem nao dizia para onde o salto
  apontava: corpo vazio, `status=302` pelado, operador sem pistas. O
  diagnostico unico (`diagnostico_de_redirecionamento`) cita o cabecalho
  `Location` sanitizado — credenciais redigidas, controles de terminal
  removidos, teto de 200 chars em fronteira de char — com sentinela para
  header ausente/vazio, e nomeia o que corrigir (a `base_url` do provider ou
  a URL pedida). O destino nunca e seguido.
