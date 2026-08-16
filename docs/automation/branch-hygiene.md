# Diretrizes de Higiene e Ciclo de Vida de Branches

Este documento estabelece as regras para criação, manutenção e exclusão de branches no repositório `michelbr84/GarraRUST`.

## 1. Princípio Fundamental: Branch Permanente Única

- A branch `main` é a **única branch permanente** de longa duração do repositório.
- Nenhuma outra branch deve ser tratada como permanente (ex.: `develop`, `staging`, branches de backup ad-hoc).
- Todas as branches de trabalho são efêmeras e devem existir apenas enquanto houver um Pull Request ativo associado.

## 2. Nomenclatura de Branches

As branches efêmeras devem seguir o padrão:

- `feat/<nome-curto>`: Novas funcionalidades.
- `fix/<nome-curto>`: Correções de bugs.
- `chore/<nome-curto>`: Manutenções, atualizações de dependências e tooling.
- `sec/<nome-curto>`: Hardening e correções de segurança.
- `docs/<nome-curto>`: Documentação.

## 3. Critério de 3 Vias para Exclusão de Branches

Uma branch remota pode e deve ser excluída se atender a pelo menos **uma** das seguintes condições comprovadas:

1. **PR correspondente mergeado**: O PR associado à branch foi mergeado com sucesso em `main`.
2. **PR correspondente fechado e descartado**: O PR associado foi fechado deliberadamente sem merge e suas alterações não serão aproveitadas.
3. **Conteúdo totalmente contido em `main`**: A comparação git (`git compare` ou `ahead_by == 0`) confirma que a branch não contém commits exclusivos que não estejam presentes em `main`.

## 4. Prevenção de Branches Órfãs

- Branches criadas por ferramentas de automação (Dependabot, bots de CI/CD, agentes) devem ser removidas automaticamente no merge (`delete_branch_on_merge = true` ativado nas configurações do repositório).
- Branches de backup ou snapshots locais nunca devem ser empurradas como referências remotas; se necessárias, devem ser mantidas localmente ou registradas como tags anotadas assinadas.
- Limpezas automatizadas periódicas devem ser executadas utilizando o workflow `.github/workflows/branch-cleanup.yml` primeiro em modo *dry-run* com validação de log antes da execução com confirmação.
