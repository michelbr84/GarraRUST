E2E do `cli_inspect` passa a provar, pelo fio e nos dois provedores, a semântica
de identidade que a #1712 exige: `provider_selected` (seleção da CLI),
`model_requested` (seleção corrente, acompanhando a troca de `/model`) e
`last_turn` (o que o turno realmente usou) são campos distintos. No caminho de
streaming — o do REPL — o relatório declara `model_confirmed_by_provider: false`
e `source: runtime_requested_model`, em vez de apresentar o modelo pedido como
confirmado pelo provedor; trocar o modelo entre turnos muda `model_requested`
sem reescrever o modelo do turno anterior.
