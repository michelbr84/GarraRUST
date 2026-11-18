- **Os rotulos da coluna `Denied by` do console admin tem paridade testada (#1915).**
  Os mapas em JS que traduzem `denied_by` / `denied_because` duplicavam os
  codigos de `CamadaDaRecusa` / `MotivoDaPolitica` sem teste, entao um codigo
  novo sairia cru sem quebrar nada. Os dois enums ganham `TODOS` (com teste de
  exaustividade) e `tests/console_rotulos_da_recusa.rs` cobra a paridade nos
  dois sentidos. Os mapas passam a ser sem prototipo: um codigo como
  `constructor` sai cru em vez de virar funcao. So exibicao e teste.
