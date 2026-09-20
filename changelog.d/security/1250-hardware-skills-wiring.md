- **O catalogo de skills de hardware passa a classificar risco em runtime —
  `kind: hardware-*` tem consumidor (#1250).** A crate de dados (manifestos
  `hardware-adapter`/`hardware-preset`, lista fechada de transportes) e a
  regra "risco efetivo = max(adapter, skill)" existiam inteiras, e nenhum
  deploy as exercitava: `spawn_hardware_adapters` nunca carregava o
  catalogo, e o registry aplicava so o teto do adapter. Agora o boot de
  hardware (gateway e `garra chat`, a mesma funcao) carrega o catalogo do
  MESMO dir de skills que o scanner de instrucoes usa antes de subir
  qualquer adapter, e injeta o elevador no registry: um device registrado
  entra embrulhado num decorator cuja visao de capabilities e a efetiva —
  leitura continua R0 (invariante de `Capability`), acao recebe o maximo
  entre adapter e preset, e `read`/`execute` passam por dentro. Skills com
  transporte fora da lista fechada (`mqtt`, `home_assistant`, `serial`,
  `gpio`) sao carregados inertes e ganham `warn!` no boot. A descoberta
  tambem mostra os apelidos: presets declaram sinonimos pt/en e o
  `device_list` lista `aliases: ...` por device, para o agente ligar
  "luz da sala" ao id que o gate ve.
  Fail-closed em todas as bordas: skills dir ausente, catalogo vazio ou
  erro de leitura/parse nao mudam nada (risco fica no teto do adapter,
  descoberta sem aliases, warn no log) — e como o elevador so sobe risco,
  um catalogo parcial (skill cujo parse falhou nao entra) nunca abaixa
  risco nenhum.
