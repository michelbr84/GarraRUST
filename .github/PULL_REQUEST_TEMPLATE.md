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

## Checklist

### Obrigatório antes de abrir o PR

- [ ] `cargo fmt --all` executado
- [ ] `cargo clippy --workspace -- -D warnings` sem erros
- [ ] `cargo test --workspace` passando
- [ ] `cargo check -p <crate-afetada>` sem erros
- [ ] Nenhum `unwrap()` adicionado em código de produção
- [ ] Nenhum secret, API key ou credencial nos arquivos

### Se aplicável

- [ ] Testes adicionados para o novo comportamento
- [ ] Documentação em `docs/src/` atualizada
- [ ] `CHANGELOG.md` atualizado (se mudança relevante para o usuário)
- [ ] `config.yml` de exemplo atualizado (se nova opção de configuração)
- [ ] Endpoint documentado em `docs/src/api-reference.md` (se novo endpoint)

## Mudanças na API pública

<!-- Liste quaisquer mudanças que quebram compatibilidade (breaking changes), novas rotas REST, novos campos de configuração, ou mudanças em traits públicos. -->

- Nenhuma mudança na API pública

## Como testar

<!-- Descreva passos específicos para verificar que o PR funciona corretamente. -->

1. Configure `~/.garraia/config.yml` com...
2. Execute `garraia start`
3. Teste via `curl http://127.0.0.1:3888/...`
4. Resultado esperado: ...

## Screenshots / logs (se aplicável)

<!-- Para mudanças visuais ou comportamentos difíceis de descrever em texto -->

## Contexto adicional

<!-- Decisões de design, trade-offs considerados, alternativas descartadas -->
