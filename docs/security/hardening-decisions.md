# Decisões de Hardening e Postura de Segurança

Este documento consolida as decisões arquiteturais de segurança e mitigação de riscos no `michelbr84/GarraRUST`.

## 1. Zero Tolerância a Vulnerabilidades Conhecidas

- O repositório mantém **zero alertas abertos** no GitHub Dependabot e zero vulnerabilidades ativas no `cargo audit`.
- Atualizações de segurança são priorizadas como caminho crítico.
- Quando uma vulnerabilidade em dependência transitiva não possui correção imediata viável (ex.: dependência upstream não atualizada), a supressão temporária exige:
  - Justificativa técnica formal documentada em `.cargo/audit.toml` e `deny.toml`.
  - Issue de tracking aberta no repositório com prazo de expiração (máximo 90 dias).
  - Análise de impacto comprovando que o caminho de código vulnerável não é alcançável em tempo de execução.

## 2. Proteção de Segredos e Push Protection

- GitHub Secret Scanning e Push Protection estão habilitados.
- Nenhuma credencial, token ou chave privada deve ser commitada no histórico ou arquivos de configuração.
- Scripts de varredura pré-commit e validações CI utilizam ferramentas especializadas (Gitleaks) e sanitização em tempo de build.

## 3. Isolamento Multi-Tenant e RLS (Row-Level Security)

- Toda persistência multi-tenant utiliza PostgreSQL 16 com políticas estritas de Row-Level Security (`FORCE RLS`).
- As transações de handlers autenticados executam obrigatoriamente `set_config('app.current_user_id', ...)` e `set_config('app.current_group_id', ...)`.
- As suítes de teste de autorização (`Security Gate (BOLA)`) exercitam isolamento entre grupos e verificação de não-vazamento de existência em rotas REST (retornando 404 em acessos não autorizados).

## 4. Proteção de Branches e Rulesets

- A branch `main` é protegida pelo Ruleset `15901595` com verificação estrita de checks de CI (Format Check, Clippy Linting, Tests Ubuntu/Windows, cargo-deny, Secret Scan).
- Merges diretos em `main` sem validação de CI são bloqueados por padrão.
