- **A compactacao da memoria passa a rodar sozinha — ate aqui nunca rodou (#956).**
  O `MemoryStore::compact()` existia desde sempre e os unicos chamadores eram os
  testes e, desde o #950, a CLI. Na pratica a memoria de longo prazo crescia sem
  teto: ruido acumulava, o recall degradava (mais candidatos no KNN, mais lixo
  entre eles) e o backup ficava maior a cada dia. Agora o gateway sobe uma
  varredura periodica (`memory.retention`) que apaga entradas nao-fixadas mais
  velhas que a janela configurada. **Nasce desligada de proposito:** liga-la por
  default numa atualizacao apagaria memoria de quem so quis atualizar a versao.
  Enquanto esta desligada o boot avisa uma vez que a memoria cresce sem teto e
  como ligar — o operador ganha o sinal sem pagar com dado.
