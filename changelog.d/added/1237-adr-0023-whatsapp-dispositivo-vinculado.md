- **ADR 0023 decide como o WhatsApp pessoal vai funcionar (#1237).** O canal
  WhatsApp de hoje e a Meta Cloud API, que exige conta Business, numero
  cadastrado na Meta e URL publica — barreira que o usuario domestico nao
  passa. O ADR registra o caminho do WhatsApp **pessoal**, por dispositivo
  vinculado com QR code, planejado para a v0.4.3 sob `garra whatsapp`: um
  bridge Node/Baileys **stateless** falando NDJSON por stdio, com o Rust dono
  do estado (sessao num blob unico AES-256-GCM, 0600 em diretorio 0700, chave
  vinda da passphrase do cofre ou de um `session.key` proprio), supervisao no
  mesmo regime do MCP (`env_clear` + allowlist de env, PDEATHSIG, RLIMIT_AS,
  filho morre no `Drop`), maquina de estados pura no molde do
  `garraia-desktop-core`, e canal pull separado (`whatsapp_linked`) com
  allowlist e pareamento obrigatorios e tools read-only por default. Duas
  consequencias ficam escritas em vez de escondidas: Node.js vira requisito
  **so desse caminho**, o que arranha o "12 canais num binario" (agora com nota
  de rodape na comparacao), e cliente nao-oficial pode levar ao bloqueio da
  conta — por isso ha tela de consentimento antes do primeiro QR e recomendacao
  de numero secundario. Alternativas avaliadas: Go/whatsmeow como asset
  estatico (adiada, mexe na superficie de assets de release, R5 do dono),
  protocolo nativo em Rust (inviavel) e reusar o daemon do OpenClaw (destroi o
  "um comando, uma tela"). Nada disso existe ainda em build publicado; o ADR e
  a decisao, nao a entrega.
