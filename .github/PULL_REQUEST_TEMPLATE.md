## Descrição

<!-- Explique o que este PR faz e por quê. Inclua contexto suficiente para que o revisor entenda sem precisar ler todos os commits. -->

Closes #<!-- número da issue relacionada, se houver -->

## Tipo de mudança

- [ ] `feat`: Nova funcionalidade
- [ ] `fix`: Correção de bug
- [ ] `refactor`: Refatoração sem mudança de comportamento
- [ ] `docs`: Documentação apenas
- [ ] `test`: Adição ou correção de testes
- [ ] `perf`: Melhoria de performance
- [ ] `chore`: Manutenção (deps, CI, build)
- [ ] `sec`: Correção ou hardening de segurança

## Classe de Risco

- [ ] **Classe A (Baixo Risco)**: Documentação, comentários, formatação, bumps de dependências de desenvolvimento sem impacto em tempo de execução.
- [ ] **Classe B (Médio Risco)**: Atualizações de dependências em produção, refatorações internas sem alteração de schema ou de API pública.
- [ ] **Classe C (Alto Risco)**: Alterações em autenticação, criptografia, RLS (Row-Level Security), migrations SQL, endpoints REST públicos, autorização/RBAC.

## Checklist de Segurança e Qualidade

### Obrigatório antes de abrir o PR

- [ ] `cargo fmt --check` executado e limpo
- [ ] `cargo clippy --workspace --exclude garraia-desktop -- -D warnings` sem erros
- [ ] `cargo test --workspace --exclude garraia-desktop` passando localmente
- [ ] Nenhum `unwrap()` adicionado em código de produção
- [ ] Nenhum segredo, API key, token ou credencial commitada
- [ ] Nenhuma migração Postgres com operações destrutivas sem estratégia de transição forward-only

### Para alterações de Alto Risco (Classe C)

- [ ] Testes de autorização cross-group/cross-tenant adicionados ou atualizados
- [ ] Verificação de políticas RLS `USING` e `WITH CHECK`
- [ ] Auditoria de eventos de workspace estruturada (sem PII na metadata)

## Mudanças na API pública e Schema

<!-- Liste quaisquer quebras de compatibilidade, novas rotas REST, novos campos de configuração ou novas migrations. -->

- Nenhuma mudança na API pública

## Como testar

<!-- Descreva passos específicos e comandos para validar a alteração. -->

1. 

## Plano de Rollback

<!-- Descreva como reverter a mudança de forma segura se um problema for detectado em produção. -->
