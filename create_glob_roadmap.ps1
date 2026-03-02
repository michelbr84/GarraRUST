# PowerShell script para criar roadmap de Glob & Ignore Engine no Linear
# Usage: .\create_glob_roadmap.ps1

$ApiKey = "REDACTED_LINEAR_API_KEY"

if ([string]::IsNullOrEmpty($ApiKey)) {
    Write-Host "Erro: Defina a variavel LINEAR_API_KEY" -ForegroundColor Red
    exit 1
}

Write-Host "Criando roadmap Glob & Ignore Engine no Linear..." -ForegroundColor Green

$headers = @{
    "Authorization" = $ApiKey
    "Content-Type" = "application/json"
}

# Buscar ID do projeto/equipe GAR
Write-Host "Buscando equipe GAR..." -ForegroundColor Cyan
$query = @{ query = "query { teams { nodes { id name key } } }" } | ConvertTo-Json
$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $query -ErrorAction Stop
$team = $response.data.teams.nodes | Where-Object { $_.key -eq "GAR" }

if (-not $team) {
    Write-Host "Equipe GAR nao encontrada!" -ForegroundColor Red
    exit 1
}

Write-Host "Equipe GAR encontrada: $($team.id)" -ForegroundColor Green

# Buscar estado "Todo" ou primeiro estado disponivel
$stateQuery = @{ query = "query { team(id: `"$($team.id)`") { states { nodes { id name } } } }" } | ConvertTo-Json
$stateResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $stateQuery -ErrorAction Stop
$todoState = $stateResponse.data.team.states.nodes | Where-Object { $_.name -eq "Todo" -or $_.name -eq "Backlog" } | Select-Object -First 1

if (-not $todoState) {
    $todoState = $stateResponse.data.team.states.nodes | Select-Object -First 1
}

Write-Host "Usando estado: $($todoState.name) ($($todoState.id))" -ForegroundColor Green

# Definir roadmap completo - Glob & Ignore Engine
$roadmap = @(
    # Epic 1.1 - Spec & UX
    @{ title = "1.1.1 - Documento Glob Semantics for GarraRUST"; desc = "Diferencas entre * e **. Regras de separador. Dotfiles. Extglob e negations. Referenciar diferencas Picomatch vs Bash."; priority = 1 },
    @{ title = "1.1.2 - Definir modo padrao do Garra"; desc = "Default = picomatch (previsivel, evita backtracking agressivo). Configuravel via config."; priority = 1 },
    @{ title = "1.1.3 - Matriz de compatibilidade"; desc = "Tabela interna: * atravessa /, ** recursivo, extglob suporte. Cada regra com exemplo e teste."; priority = 2 },
    
    # Epic 2.1 - Glob Engine API
    @{ title = "2.1.1 - Criar API publica garraia-glob"; desc = "GlobMode: Picomatch, Bash. GlobOptions: dot, case_sensitive, path_separator_policy, max_pattern_bytes."; priority = 1 },
    @{ title = "2.1.2 - Normalizacao de paths"; desc = "Converter paths para formato interno. Garantir matching em relative paths."; priority = 1 },
    @{ title = "2.1.3 - Safe-guards de performance"; desc = "Limites de pattern, profundidade maxima, limite de alternativas em brace/extglob. Evitar explosao."; priority = 1 },
    
    # Epic 2.2 - Picomatch Mode
    @{ title = "2.2.1 - Matching de * vs **"; desc = "* nao atravessa /. ** atravessa multiplos diretorios (recursivo)."; priority = 1 },
    @{ title = "2.2.2 - Extglob sem backtracking caro"; desc = "Replicar comportamento Picomatch para !(...) sem greedy backtracking."; priority = 1 },
    @{ title = "2.2.3 - Suite de testes do modo Picomatch"; desc = "Arquivo com casos: nested dirs, dotfiles, escapes, extglob basicos, negations e precedencia."; priority = 1 },
    
    # Epic 2.3 - Bash Mode
    @{ title = "2.3.1 - Implementar extglob completo Bash-style"; desc = "Avaliar zlob com ZLOB_EXTGLOB. AC: !(...) @(...) ?(...) *(...) +(...) funcionam em paths."; priority = 2 },
    @{ title = "2.3.2 - Politica de backtracking"; desc = "Default seguro: sem explosao de memoria. Flag opcional: bash_greedy_negated_extglob=true."; priority = 2 },
    @{ title = "2.3.3 - Suite de testes do modo Bash"; desc = "Casos cobrindo globstar e extglob."; priority = 2 },
    
    # Epic 3.1 - Ignore System
    @{ title = "3.1.1 - Implementar leitor de .garraignore"; desc = "Sintaxe: # comment, glob, !negation, / root-anchored. Prioridade: --include > .garraignore > .gitignore > defaults."; priority = 1 },
    @{ title = "3.1.2 - Explain ignore"; desc = "Dado um path, retornar qual arquivo e pattern decidiu. Comando/debug retorna motivo completo."; priority = 2 },
    
    # Epic 3.2 - Coherence
    @{ title = "3.2.1 - Definir como .garraignore interpreta patterns"; desc = "Opcao A: usa regras gitignore/ignore. Opcao B: respeita glob_mode."; priority = 2 },
    @{ title = "3.2.2 - Testes de precedencia"; desc = "Re-include com !, pastas ignoradas com excecoes, interacao com include/exclude."; priority = 2 },
    
    # Epic 4.1 - Indexer
    @{ title = "4.1.1 - Scanner do repo respeitando ignore + glob_mode"; desc = "Entrada: root path, include globs, exclude globs, .garraignore/.gitignore, mode. Saida: arquivos + stats."; priority = 1 },
    @{ title = "4.1.2 - Log de cobertura"; desc = "Expor: top reasons de exclusao, top patterns que excluíram, contagem por extensao."; priority = 2 },
    
    # Epic 4.2 - Watchers
    @{ title = "4.2.1 - Filtro de eventos do watcher"; desc = "Antes de reagir a file event, passar no matcher. Evitar rebuild/index em target/, node_modules/."; priority = 1 },
    @{ title = "4.2.2 - Debounce inteligente por glob group"; desc = "Ex.: alteracoes em **/*.rs agrupam em 1 evento. Ajustavel por config."; priority = 2 },
    
    # Epic 4.3 - CLI
    @{ title = "4.3.1 - Config em config.basic.yml"; desc = "fs.glob.mode: picomatch|bash, fs.glob.dot: true|false, fs.ignore.use_gitignore: true|false."; priority = 1 },
    @{ title = "4.3.2 - Flags CLI"; desc = "--glob-mode, --include **/*.rs (multi), --exclude **/target/** (multi), --ignore-file."; priority = 1 },
    @{ title = "4.3.3 - Comando test glob"; desc = "garra glob test --mode --pattern --path. Retorna match + reason."; priority = 1 },
    
    # Epic 4.4 - Web UI
    @{ title = "4.4.1 - Tela de settings File Matching"; desc = "Dropdown glob mode, toggles dotfiles/gitignore, textarea include/exclude, live tester."; priority = 2 },
    
    # Epic 5.1 - Tests
    @{ title = "5.1.1 - Test vectors baseados em casos conhecidos"; desc = "Casos do README Picomatch (nested dirs e !(foo)*). Casos do Bash manual/extglob."; priority = 1 },
    
    # Epic 5.2 - Benchmarks
    @{ title = "5.2.1 - Benchmark de matching"; desc = "10k/100k/200k paths, padroes simples vs extglob, comparar picomatch vs bash."; priority = 2 },
    @{ title = "5.2.2 - Benchmark de traversal"; desc = "Usando ignore WalkBuilder (com e sem gitignore)."; priority = 2 },
    
    # Epic 5.3 - Security
    @{ title = "5.3.1 - Protecoes anti pattern bombs"; desc = "Caps de backtracking, caps de expansao braces/extglob, recusar patterns grandes, log."; priority = 1 },
    
    # Epic 6.1 - User Docs
    @{ title = "6.1.1 - Pagina Globs no Garra"; desc = "Exemplos: **/*.rs, **/*.{ts,tsx}, excluir target/ e node_modules/, diferenca entre * e **."; priority = 2 },
    @{ title = "6.1.2 - Migration guide"; desc = "Se hoje usam algum matcher implicito, documentar mudancas."; priority = 2 },
    
    # Epic 6.2 - Internal Docs
    @{ title = "6.2.1 - Como adicionar novos filtros / usar o matcher"; desc = "Snippet de uso da API garraia-glob. Guidelines: nunca filtrar paths na mao."; priority = 2 }
)

# Criar issues
$created = 0
$failed = 0

foreach ($item in $roadmap) {
    Write-Host "Criando: $($item.title)..." -ForegroundColor Cyan
    
    $mutation = @{
        query = "mutation CreateIssue(`$issue: IssueCreateInput!) { issueCreate(input: `$issue) { success issue { id identifier } } }"
        variables = @{
            issue = @{
                teamId = $team.id
                stateId = $todoState.id
                title = $item.title
                description = $item.desc
                priority = $item.priority
            }
        }
    } | ConvertTo-Json -Depth 5
    
    try {
        $resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $mutation -ErrorAction Stop
        if ($resp.data.issueCreate.success) {
            Write-Host "  OK: $($resp.data.issueCreate.issue.identifier)" -ForegroundColor Green
            $created++
        } else {
            Write-Host "  FALHA: $($item.title)" -ForegroundColor Red
            $failed++
        }
    } catch {
        $errMsg = $_.Exception.Message
        if ($errMsg -like "*duplicate*") {
            Write-Host "  JA EXISTE (pulando)" -ForegroundColor Yellow
        } else {
            Write-Host "  ERRO: $errMsg" -ForegroundColor Red
        }
        $failed++
    }
}

Write-Host ""
Write-Host "=== Resultado ===" -ForegroundColor Green
Write-Host "Criadas: $created" -ForegroundColor Green
Write-Host "Falhas: $failed" -ForegroundColor Red
Write-Host ""
Write-Host "Roadmap Glob & Ignore Engine criado com sucesso!" -ForegroundColor Green
