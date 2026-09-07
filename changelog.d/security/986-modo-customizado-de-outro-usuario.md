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
- **`PATCH` e `DELETE /api/modes/custom/{id}` tambem passam a respeitar o dono.**
  A primeira versao deste trabalho deu escopo so ao `GET`, usando para a leitura
  um argumento que vale **mais** para a escrita: sobrescrever ou apagar o modo de
  outra pessoa e pior que le-lo. O escopo entra na clausula `WHERE` das duas
  consultas, e nao num filtro depois — um `UPDATE` que casa a linha alheia ja
  escreveu quando o filtro rodaria. Como o resto, hoje e inerte e existe para o
  dia em que houver identidade de verdade.
- O `format!` que monta a lista de colunas do `UPDATE` ganhou o comentario de
  auditoria que a regra absoluta 5 exige na sua propria excecao: os fragmentos
  sao literais Rust escritos no bloco, nenhum vem de request, e todo valor
  continua indo por `?`. Sem o comentario, a proxima pessoa a acrescentar coluna
  ali nao tem o sinal de alerta.
