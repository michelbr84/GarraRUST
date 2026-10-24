- **`/api/diagnostics` deixa de acender amarelo numa instalacao nova (#1695).**
  A linha `tools.capabilities` saia `warning` em toda instalacao limpa,
  porque `device_read`/`device_execute` (sem adapter de hardware) e
  `telegram_send_voice` (modo voz desligado) contavam como indisponiveis —
  visto no Windows e no Linux limpos do v0.4.10-rc1, contra o criterio D9 do
  gate de release ("sem nenhum aviso espurio"). Agora ausencia de
  configuracao vira `not_configured` (neutro, como o `bash` sem sandbox desde
  a #1471): as tools de hardware devolvem `not_configured` quando nenhum
  adapter (`hardware.mqtt`/`hardware.home_assistant`) foi declarado, e o
  registro de capacidades trata `tts_unconfigured` como nao configurado.
  Configurado e quebrado continua amarelo: adapter declarado sem nenhum
  dispositivo segue `no_devices`, e canal configurado fora do ar segue
  `channel_offline`. O motivo de origem continua no `reason_code`.
