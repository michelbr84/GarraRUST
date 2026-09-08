- **`DELETE /api/memory/{id}` e apagar memoria pelo app (#1043).** Da para buscar
  na memoria, nao dava para apagar uma: `MemoryStore::delete_entry` existia e so
  a CLI usava; o `/api/*` do celular so tinha `DELETE /api/memory`, que apaga a
  sessao inteira. `MemoryProvider` ganha `delete_entry`, a rota devolve 204/404/
  503, e a folha de memoria do app ganha Apagar com confirmacao (aviso extra
  quando a entrada esta fixada, porque o store nao consulta o pin). Editar fica
  de fora: sem update no store, seria apagar-e-recriar com re-embedding.
