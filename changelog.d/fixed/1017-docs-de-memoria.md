- **Os docs de memoria contradiziam o binario, em dois arquivos.**
  `docs/src/memory.md` afirmava "**Nao ha `garra memory add`**" — falso desde
  que o comando entrou (#958). E `docs/memory.md`, para onde **o wiki do
  projeto aponta duas vezes** como "Sistema de memoria", ainda descrevia o
  sistema que nunca foi construido: um `facts.json` com array de fatos
  datados, as chaves `auto_extract` e `max_facts`, e os comandos
  `garraia memory clear`, `export` e `disable`. A reescrita do #963 conferiu
  cada afirmacao contra o codigo, mas conferiu a pagina do book; esta copia
  ficou para tras e seguiu sendo servida a quem chegava pelo wiki.
- `docs/memory.md` vira redirecionamento para a pagina viva — apaga-lo
  quebraria os links do wiki. `docs/src/memory.md` ganha a secao do
  `garra memory add`, com o que importa: o embedding e gerado **na hora**,
  porque uma entrada sem vetor nao aparece na busca semantica e um comando de
  semear que deixasse a entrada invisivel ate um segundo comando seria uma
  armadilha.
