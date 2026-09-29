- **`restart` para de criar daemon orfao quando o gateway roda sob systemd (#1542).**
  `garraia restart` matava o daemon da unit e subia um processo novo por fora
  dela: o orfao ficava com a porta e a unit entrava em crash-loop contra
  `Address already in use` — 4606 reinicios em ~8h numa instalacao 0.4.6, com o
  log parecendo um MCP reconectando sem parar. A CLI agora le o cgroup de quem
  esta na porta e, se for uma unit systemd, recusa **antes** de parar qualquer
  coisa (exit 78), nomeando a unit e o `systemctl [--user] restart` certo.
  Recusa em vez de delegar porque a unit sobe pelo `ExecStart` dela e as flags
  da invocacao (`--host`/`--port`/`--with-voice`) sumiriam em silencio.
  Escotilha: `GARRAIA_ALLOW_SYSTEMD_RESTART=1`.
