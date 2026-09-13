- Hardware skills (#1131): adapters e presets de hardware passam a ser
  empacotados como skill, fechando a arquitetura em camadas do epic #1124 —
  core sem driver, integracao distribuivel. O frontmatter ganha `kind`
  (`instruction` por default, `hardware-adapter`, `hardware-preset`) e o bloco
  `provides` (transporte, capabilities, presets entidade->capability com
  sinonimos pt/en); a varredura de skills passa a descer em subdiretorios
  (`hardware/<slug>/SKILL.md`), ignorando symlinks e com teto de profundidade.
  Duas regras que o manifesto nao escolhe: a lista de transportes e fechada
  (`mqtt`, `home_assistant`, `serial`, `gpio` — qualquer outro carrega inerte,
  visivel e nunca ativo) e um skill so SOBE risco, nunca baixa (o risco
  efetivo e `max(adapter, skill)`, e leitura continua R0). Seis skills
  oficiais versionados em `skills/hardware/` — Home Assistant, MQTT,
  serial/Arduino, ESP32, Zigbee e Matter, os dois ultimos como preset sobre o
  hub em vez de stack propria. Documentado em `docs/hardware-skills.md`.
