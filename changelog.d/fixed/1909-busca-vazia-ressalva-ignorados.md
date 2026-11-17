- **`repo_search`: o resultado vazio do ripgrep diz o que ficou de fora
  (#1909).** O `rg` pula o que `.gitignore`/`.ignore` cobrem e os arquivos
  ocultos, mas a resposta era so "No matches found in ..." — lida como "nao
  existe neste diretorio" mesmo quando o texto estava num arquivo ignorado
  ou oculto. O vazio do `rg` agora acrescenta essa ressalva. Os fallbacks
  (`grep -r`, `findstr /s`) leem tudo e por isso nao a carregam. Nenhum
  argumento de busca, raiz, jail ou modo mudou.
