- **Auditoria JSONL do `never-confirm` cobre `run_tests` e `device_execute`
  (#1858).** Alem do `bash`, as suites e as acoes de hardware executadas sem
  pedido de confirmacao viram linha em `<data_dir>/audit/never-confirm.jsonl`
  (no `garra chat` e no gateway). O `device_execute` grava dispositivo,
  capability e risco, nunca os `args`. O append agora espera a escrita
  (`flush`) antes de voltar: sem isso, o `tokio::fs::File` podia gravar a
  linha depois do retorno e fora de ordem com o registro seguinte.
