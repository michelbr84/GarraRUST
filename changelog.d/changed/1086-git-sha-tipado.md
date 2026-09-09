- O sha de rollback de skill passa a ser um tipo (`GitSha`) construido a partir
  do alfabeto aceito, em vez de uma string apenas inspecionada. Na pratica o
  valor entregue ao `git` e um que o modulo montou, e nao um que ele so olhou:
  ninguem consegue mais passar string nao checada onde se espera um sha, e o
  panico de fronteira de caractere fica impossivel por construcao em vez de
  apenas barrado. Isso tambem fecha o alerta CodeQL #166, que continuou aberto
  depois da primeira correcao — e corretamente, pelo modelo dele: a validacao
  devolvia `()` e a string original seguia para `Command::args`, entao nada no
  fluxo de dados tinha mudado (#1086).
