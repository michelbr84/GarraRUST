- **O `deep-research-report.md` renderizava 98 blocos de lixo.** O documento
  entrou no repositorio em 2026-04-13 com os marcadores internos de citacao da
  ferramenta que o gerou ainda embutidos — sequencias delimitadas por
  caracteres da Private Use Area do Unicode (U+E200/E201/E202), que o GitHub
  desenha como tofu e nenhum leitor consegue seguir: `turn1search3` referencia
  um resultado de busca efemero da sessao que produziu o texto, morto desde
  entao. Eram 93 marcadores de citacao, removidos.
- Os outros 5 marcadores eram de tipo diferente e **envolviam texto de
  verdade**: apagar os spans sem olhar teria excluido as palavras `Brasil`,
  `ANPD`, `Uniao Europeia`, `NIST` e `European Data Protection Board` das
  frases em que aparecem. Foram substituidos pelo nome de exibicao. A
  conferencia foi por palavra: 5066 antes, 5066 depois, zero diferencas.
- O relatorio ganhou um **cabecalho de contexto** que ele nunca teve. Nao tinha
  data nem status, e o `CLAUDE.md` o importa como "base arquitetural da Fase
  3" — o que convidava a ler como especificacao viva uma pesquisa de abril. O
  cabecalho diz o que ele e, de quando e, que o ADR vence onde divergirem, e
  quais dos "itens nao especificados" ja foram decididos (ADR 0003, 0004 e
  0005).
- **Sete links relativos apontavam para arquivos que nao existem.** Cinco eram
  para `benches/database-poc/`, o PoC removido em 2026-08-16 pelo #814 — que
  ja tinha estabelecido o tratamento ("links mortos viram mencao historica"),
  mas corrigiu README e ROADMAP e deixou passar o ADR 0003, onde estava a
  maioria deles. A tabela B1-B5 continua reproduzida no proprio ADR, entao
  nenhum numero se perdeu; os arquivos seguem recuperaveis em `2188751^`.
- Os outros dois eram do ADR 0009, para `plans/0116a-*` e `plans/0116b-*`.
  Conferido: esses planos **nunca existiram** em ponto nenhum da historia do
  repositorio. Viraram nome de registro, com ponteiro para o plano que existe.
- `docs/src/SUMMARY.md`, o indice do mdBook, mandava para `./installation.md`
  e `./configuration.md`; os dois arquivos vivem em `docs/`, nao em
  `docs/src/`. As paginas "Instalacao" e "Configuracao" do livro sairiam
  vazias. Corrigido para `../`, que e o que o proprio SUMMARY ja usa para a
  persona da Hera. Mesmo erro em `docs/src/continue-modes.md`.
