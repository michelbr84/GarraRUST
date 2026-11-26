- **`admin.db` que existe e nao abre recusa o boot em vez de virar um console
  vazio em memoria (#2000).** O store em memoria nasce sem usuarios, e o
  `POST /admin/api/setup` cria o primeiro admin sem autenticacao enquanto nao
  ha usuario: um `admin.db` corrompido, sem permissao ou com o disco cheio
  entregava o console de uma instalacao ja configurada a quem chegasse primeiro
  em `/admin`, e o que o operador gravasse nesse boot sumia no restart. Agora o
  gateway recusa o boot, nomeando o arquivo e a saida (corrigir
  permissao/espaco ou restaurar de backup). Arquivo ausente (primeira
  instalacao) segue como antes. O `setup` tambem passa a recusar (503) quando
  nao consegue contar os usuarios, em vez de ler o erro como "zero".
