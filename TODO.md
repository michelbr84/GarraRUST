# TODO

Status operacional do backlog do GarraIA/GarraRUST. Este arquivo complementa
`ROADMAP.md`: o roadmap guarda a direção de produto; este TODO registra o que
foi concluído, o que ficou parcial ou adiado, decisões tomadas e próximos passos
curtos para a próxima sessão autônoma.

**Atualizado:** 2026-05-24 (America/New_York)

## Concluído nesta sessão

- Criado `TODO.md`, que estava ausente em `main` apesar de ser obrigatório no
  fluxo de trabalho atual.
- Sincronizado `ROADMAP.md` com o estado pós-merge de GAR-493/ADR 0011:
  PR #492 foi mergeado em `main` (`95618d3`), GAR-493 está Done e GAR-492 está
  em In Review para triagem de fechamento/follow-ups.
- Atualizado `plans/README.md`: plan 0176 agora aponta para o merge real via
  PR #492 (`95618d3`) em vez de `In Progress`.
- Reconciliada a checklist GAR-603 Runpod no `ROADMAP.md` com evidência já
  presente em código/docs: `Dockerfile`, `router.rs` e
  `docs/deployment-runpod.md`.
- Atualizado `docs/deployment-runpod.md` para explicitar que a checklist foi
  reconciliada por evidência estática e que o smoke público Runpod continua
  pendente.

## Parcialmente concluído

- GAR-603 Runpod Load Balancer Serverless:
  - Concluído por evidência estática/docs: `garra start` em modo HTTP,
    container bindando `0.0.0.0`, rotas `/ping` e `/health`, `PORT`/`HOST`,
    Dockerfile sem REPL, receita local Docker, settings Runpod e distinção
    queue-based vs Load Balancer.
  - Pendente: smoke Docker local nesta sessão e smoke público
    `https://<ENDPOINT_ID>.api.runpod.ai/ping`.
  - Pendente técnico: suporte a `PORT_HEALTH` separado quando a health port
    precisar diferir de `PORT`; hoje a documentação exige `PORT_HEALTH=PORT`.

## Adiado com justificativa

- GAR-372 / Fase 2.1 RAG embeddings: adiado porque a próxima entrega real
  exige toolchain Rust e testes; o ambiente local desta sessão não tinha
  `cargo`, `rustc` ou `rustfmt`.
- GAR-374 / Object storage S3-compatible validation: adiado por depender de
  MinIO/S3/R2/GCS ou CI com serviço externo configurado.
- GAR-410 / CredentialVault final: adiado por ser item crítico de segurança,
  amplo e inadequado para alteração sem toolchain local e validação profunda.
- GAR-504 / benchmark evidence run: adiado por depender de infra externa
  (droplet/host dedicado).
- Execução async/provider-backed das native skills GarraMaxPower: adiada para
  slice próprio após decidir o fechamento do épico GAR-492.

## Novas pendências encontradas

- O repositório não tinha `TODO.md`; manter este arquivo atualizado em toda
  sessão autônoma daqui para frente.
- O ambiente local tinha `git`, `node` e `rg`, mas não tinha `cargo`,
  `rustc`, `rustfmt`, `gitleaks` ou `markdownlint`. Mudanças de runtime devem
  esperar toolchain local ou depender explicitamente de CI no PR.
- `ROADMAP.md` ainda contém vários itens antigos marcados como `[ ]` que podem
  estar parcialmente entregues por PRs anteriores. Próxima limpeza deve
  reconciliar apenas itens com evidência clara para evitar falsear status.
- `GAR-492` está em In Review: decidir se fecha como MVP completo ou se mantém
  aberto somente até abrir follow-ups separados.

## Decisões tomadas

- Não alterar runtime Rust nesta sessão: sem toolchain local, o caminho seguro
  foi documentação, rastreabilidade e reconciliação de backlog.
- Marcar GAR-603 como parcialmente concluído, não totalmente fechado: a
  implementação/documentação está presente, mas falta prova operacional recente
  em Docker e Runpod público.
- Criar `TODO.md` como backlog operacional curto, evitando sobrecarregar
  `ROADMAP.md` com detalhes de sessão.

## Próximos passos recomendados

1. Rodar smoke Docker GAR-603:
   `docker build -t garraia:local .`,
   `docker run --rm -p 3888:3888 garraia:local`,
   `curl -fsS http://localhost:3888/ping`,
   `curl -fsS http://localhost:3888/health`.
2. Rodar smoke público Runpod quando houver endpoint disponível:
   `curl -fsS https://<ENDPOINT_ID>.api.runpod.ai/ping`.
3. Abrir follow-up para `PORT_HEALTH` separado somente se Runpod exigir health
   listener distinto de `PORT`.
4. Decidir destino de GAR-492: fechar épico como MVP completo ou abrir issues
   separadas para dogfood em bug real e execução async/provider-backed.
5. Preparar ambiente local com Rust toolchain para permitir mudanças de código
   mais ambiciosas nas próximas sessões.
