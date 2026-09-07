- **`GET /api/modes/custom/{id}` deixa de ler o modo de qualquer usuario.** A
  consulta filtrava so por `id`, e a tabela tem `user_id` — entao o endpoint
  devolvia o `prompt_override` de outra pessoa. Hoje o buraco e inerte, porque
  todo modo e gravado sob a mesma identidade e o `/api/*` e auth-free por
  desenho (local-first, mono-usuario); mas o #986 faz a **execucao** passar a
  depender da resolucao de modo customizado, e ligar isso a uma busca sem escopo
  transformaria um buraco inerte em caminho ativo. O handler passa a usar
  `get_custom_mode_for_user`, a resolucao de execucao procura por nome dentro de
  `get_custom_modes(user_id)`, e o `user_id` fixo deixa de ser uma string
  repetida em quatro lugares para virar uma constante nomeada — de modo que quem
  trocar por identidade real troque num lugar so. Teste cross-user confirma que
  nem o `GET` por id, nem a listagem, nem o `select` alcancam o modo alheio.
