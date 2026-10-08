# GarraIA — Política de Depreciação — PROPOSTA

> **Status: PROPOSTA de política — aguarda ratificação do maintainer**
> (issue de GA de operação).

## Proposta

1. **Aviso antes da remoção**: qualquer funcionalidade, canal, config ou
   endpoint será anunciado como deprecado em pelo menos **um release** antes
   da remoção, na categoria `deprecated` do CHANGELOG, com a alternativa
   indicada.
2. **Prazo mínimo**: a remoção só pode entrar em **minor ou major**
   seguinte (nunca em patch), respeitando a janela da
   `docs/operations/version-support-policy.md`.
3. **Estado "wiring pendente" ≠ deprecado**: canais implementados no crate
   com wiring pendente (ver ROADMAP) não estão deprecados — são trabalho não
   concluído. Se algum for abandonado de fato, entra no processo acima.
4. **Deprecado ≠ quebrado**: versão deprecada continua funcionando durante a
   janela de aviso; o produto pode emitir aviso de depreciação em runtime
   (ex.: `garraia doctor`) quando fizer sentido.
5. **Remoção**: commit com menção ao issue/fragmento `deprecated`→`removed`
   no CHANGELOG da release que remove.

## Por que existe

O `changelog.d/deprecated/` já existe como categoria — a estrutura de registro
está no lugar; falta a política para que o registro seja cumprido.
