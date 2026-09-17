- **A isencao por `.` da redacao de stderr entregava a chave inteira
  (#1238).** A regra poupava uma sequencia longa quando ela continha `.` ou
  `@`, marcas que nenhum alfabeto base64 produz. So que `.` e `=` estavam os
  dois DENTRO da sequencia, entao `state.creds=<chave>` era UMA sequencia so:
  o ponto do nome vizinho a isentava e a credencial saia junto. E o mesmo
  mecanismo ja documentado para `-`/`_` — "um nome de chave vizinho cola na
  sequencia" —, que so nao tinha sido aplicado ao `.`. Medido sobre 2000
  chaves de 32 B por celula, nos dois alfabetos: `creds.noiseKey=<chave>` e
  `at state.creds=<chave>` entregavam a chave INTEIRA em 100% dos casos,
  enquanto os dois contextos ja medidos (`noiseKey=` e `npm ERR! _auth=`)
  seguiam em 0%. A forma nao vem do `bridge.mjs`, que nao tem `console.*`;
  vem de um `throw` de dentro do Baileys, de modulo de terceiro ou de
  template literal com caminho de propriedade. Agora `.` e `@` sao
  separadores e nao marcas de isencao: a isencao vale por SEGMENTO, e os
  quatro contextos caem para 0%, com o maior pedaco em claro em ~1,3
  caractere. A lista de separadores nao cresce alem desses dois, porque cada
  candidato quebra um alfabeto inteiro — medido, `-`/`_` separando entregam
  ~62% das chaves url-safe e `/` separando entrega ~35% das chaves do base64
  padrao. O preco, medido sobre um corpus de 14 linhas reais de erro de
  Node/npm: 10 saem identicas e 4 perdem UM segmento do meio do caminho; em
  tres delas some o prefixo de instalacao e fica a parte informativa
  (`@whiskeysockets/baileys/lib/index.js:42:7`), e numa delas o segmento de
  40 caracteres que some e o proprio nome do modulo. O novo caso ganhou teste
  de ponta a ponta no call site, com processo de verdade.
- **A nota sobre os limites da varredura do fonte prometia completude que a
  medicao desmente (#1238).** Ela dizia que `#[serde(transparent)]` era "o
  unico caminho" que nem a allowlist de `expose()` nem a varredura de macro
  enxergam. Sao pelo menos tres, e os outros dois foram plantados e medidos:
  um `impl Deref<Target = str>` mais um `let cru: &str = b;` logado noutro
  arquivo passa 107/107 verdes, e o campo cru renomeado dentro do proprio
  `session.rs` (`let cru = &self.0;` e depois `%cru`) tambem passa 107/107 —
  o `.0` da lista de proibidos so e consultado dentro do bloco da macro, entao
  o controle `%self.0` direto ficava vermelho e o renomeado passava. Nenhum
  dos dois e vazamento vivo: os dois exigem codigo novo escrito e revisado. A
  familia certa e "qualquer conversao de tipo que produza `&str`", e a nota
  agora diz isso. Fecharam-se os dois: os `impl` que mencionam `SessionBlob`
  e o derive do tipo passaram a ter allowlist de DECLARACAO — porque a
  conversao acontece no tipo, e o call site que a dispara e indistinguivel de
  qualquer outro `let` — e o campo cru ganhou a mesma regra invertida que o
  `expose()` ja tinha, com allowlist por linha inteira. A serializacao
  transparente segue sendo buraco conhecido, de proposito e por escrito.
