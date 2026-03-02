# PowerShell script para criar roadmap Glob & Ignore Engine v2 (Telegram-first)
# Reorganizado com Priority 0: Corrigir Telegram primeiro

$ApiKey = "REDACTED_LINEAR_API_KEY"

Write-Host "Criando roadmap Glob & Ignore Engine v2 (Telegram-first)..." -ForegroundColor Green

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

# Buscar estado "Backlog" para novas issues
$stateQuery = @{ query = "query { team(id: `"$($team.id)`") { states { nodes { id name } } } }" } | ConvertTo-Json
$stateResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $stateQuery -ErrorAction Stop
$backlogState = $stateResponse.data.team.states.nodes | Where-Object { $_.name -eq "Backlog" } | Select-Object -First 1

if (-not $backlogState) {
    $backlogState = $stateResponse.data.team.states.nodes | Select-Object -First 1
}

Write-Host "Usando estado: $($backlogState.name) ($($backlogState.id))" -ForegroundColor Green

# Roadmap v2 - Telegram-first approach
$roadmap = @(
    # ============================================
    # INITIATIVE 0 - P0: CORRIGIR TELEGRAM PRIMEIRO
    # ============================================
    
    # Epic 0.1 - Reproduzir e isolar o bug
    @{ title = "0.1.1 - Criar fixture repo de teste (paths + subpastas)"; desc = "Criar estrutura: src/, src/components/, test/, target/, dotfiles. Arquivo glob_cases.json com casos minimos: *, **/*.rs, !(foo)*. Testes automatizados para reproduzir comportamento atual do Telegram."; priority = 0; labels = "area/telegram,type/story,prio/p0" },
    @{ title = "0.1.2 - Instrumentacao no Telegram"; desc = "Logar: glob_mode, root path, path normalization, include/exclude. Adicionar comando admin /diag_glob <pattern> <path> que retorna match + reason."; priority = 0; labels = "area/telegram,type/story,prio/p0" },
    
    # Epic 0.2 - Corrigir comportamento (sem depender de Bash)
    @{ title = "0.2.1 - Implementar garraia-glob minimo (modo Picomatch-like)"; desc = "Regras base: * nao atravessa /, ** atravessa diretorios. Negacao !(...) sem backtracking agressivo. Engine: glob-match/fast-glob (matching linear). Telegram passa a usar esse motor."; priority = 0; labels = "area/telegram,area/fs,type/story,prio/p0" },
    @{ title = "0.2.2 - Normalizacao de path (Windows/Linux)"; desc = "Internamente padronizar separador /. Matching sempre em paths relativos ao root do workspace/projeto."; priority = 0; labels = "area/telegram,area/fs,type/story,prio/p0" },
    @{ title = "0.2.3 - File IO correto no Telegram"; desc = "Garantir que toda leitura/gravacao passe por: validacao nao sai do root (.., symlink policy), canonicalizacao segura. Telegram nao falhar nem escrever fora do lugar."; priority = 0; labels = "area/telegram,area/fs,type/story,prio/p0,risk/security" },
    
    # Epic 0.3 - Validacao cruzada Web UI vs Telegram
    @{ title = "0.3.1 - Teste de paridade Web UI vs Telegram"; desc = "Mesmos inputs -> mesma lista final de arquivos em Web UI e Telegram (para o mesmo root/config). AC: parity test verde em CI."; priority = 0; labels = "area/telegram,area/webui,type/story,prio/p0" },
    
    # ============================================
    # INITIATIVE 1 - SPEC & UX OFICIAL
    # ============================================
    
    # Epic 1.1 - Documento Glob Semantics
    @{ title = "1.1.1 - Spec completa Glob Semantics"; desc = "* vs **, dotfiles, escapes, separador, precedencia include/exclude. Extglob e negations + diferencas Picomatch vs Bash (ex: !(foo)*). AC: cada regra tem exemplo + teste."; priority = 1; labels = "area/docs,type/story,prio/p1" },
    @{ title = "1.1.2 - Definir default oficial"; desc = "Default: picomatch. Bash: advanced/compat, com warning e guardrails."; priority = 1; labels = "area/fs,type/story,prio/p1" },
    
    # ============================================
    # INITIATIVE 2 - NUCLEO RUST: GARRAIA-GLOB
    # ============================================
    
    # Epic 2.1 - API publica
    @{ title = "2.1.1 - Tipos e contratos (GlobMode, GlobOptions, GlobSet)"; desc = "GlobMode { Picomatch, Bash }, GlobOptions { dot, case_sensitive, path_separator_policy, limits }, GlobSet (includes/excludes) + MatchDecision { decision, reason }."; priority = 1; labels = "area/fs,type/story,prio/p1" },
    @{ title = "2.1.2 - Guardrails de performance"; desc = "Limites de tamanho de pattern / quantidade de alternativas / profundidade. Sem exponential backtracking."; priority = 1; labels = "area/fs,risk/perf,type/story,prio/p1" },
    
    # ============================================
    # INITIATIVE 3 - IGNORE SYSTEM
    # ============================================
    
    # Epic 3.1 - Traversal robusto com ignore
    @{ title = "3.1.1 - WalkBuilder integrado"; desc = "Respeitar .gitignore e custom ignore. Overrides programaticos (include/exclude via config/CLI). AC: scan rapido e consistente."; priority = 1; labels = "area/fs,type/story,prio/p1" },
    @{ title = "3.1.2 - .garraignore"; desc = "Sintaxe estilo gitignore (inclui !re-include). Prioridade: 1) overrides CLI/config, 2) .garraignore, 3) .gitignore (toggle), 4) defaults (target/, node_modules/, .git/, etc)."; priority = 1; labels = "area/fs,type/story,prio/p1" },
    @{ title = "3.1.3 - Explain ignore"; desc = "Mostra arquivo/pattern responsavel + se houve negation."; priority = 2; labels = "area/fs,type/story,prio/p2" },
    
    # ============================================
    # INITIATIVE 4 - INTEGRACOES
    # ============================================
    
    # Epic 4.1 - Indexer / Memoria (RAG)
    @{ title = "4.1.1 - Scanner unificado"; desc = "Input: root + includes/excludes + ignore + mode. Output: lista de arquivos + stats + logs de cobertura."; priority = 1; labels = "area/indexer,type/story,prio/p1" },
    
    # Epic 4.2 - Watchers
    @{ title = "4.2.1 - Filtro de eventos via GlobSet"; desc = "Antes de reagir a file event, passar no matcher. Evitar rebuild/index em target/, node_modules/."; priority = 2; labels = "area/watcher,type/story,prio/p2" },
    @{ title = "4.2.2 - Debounce por grupo de glob"; desc = "Agrupar eventos por padrao glob (ex: alteracoes em **/*.rs)."; priority = 2; labels = "area/watcher,type/story,prio/p2" },
    
    # Epic 4.3 - CLI + Config
    @{ title = "4.3.1 - config.basic.yml"; desc = "fs.glob.mode, fs.glob.dot, fs.ignore.use_gitignore."; priority = 1; labels = "area/cli,type/story,prio/p1" },
    @{ title = "4.3.2 - Flags CLI"; desc = "--glob-mode, --include, --exclude, --ignore-file."; priority = 1; labels = "area/cli,type/story,prio/p1" },
    @{ title = "4.3.3 - garra glob test"; desc = "Retorna match + reason."; priority = 1; labels = "area/cli,type/story,prio/p1" },
    
    # Epic 4.4 - Web UI
    @{ title = "4.4.1 - Tela File Matching + live tester"; desc = "Dropdown glob mode, toggles, textarea include/exclude, live tester."; priority = 2; labels = "area/webui,type/story,prio/p2" },
    
    # ============================================
    # INITIATIVE 5 - BASH-LIKE MODE (OPCIONAL)
    # ============================================
    
    # Epic 5.1 - Extglob real
    @{ title = "5.1.1 - Implementar Bash-like extglob"; desc = "Opcao tecnica: zlob (feature ZLOB_EXTGLOB). Documentar que extglob e comportamento ligado a opcao do shell no Bash. AC: !(...) @(...) ?(...) *(...) +(...) funcionam."; priority = 2; labels = "area/fs,type/story,prio/p2" },
    @{ title = "5.1.2 - Politica de backtracking e safety"; desc = "Default seguro. Flag explicita (compat 100%): com caps e warning."; priority = 2; labels = "area/fs,risk/perf,type/story,prio/p2" },
    
    # ============================================
    # INITIATIVE 6 - QUALIDADE
    # ============================================
    
    # Epic 6.1 - Golden tests
    @{ title = "6.1.1 - Test vectors (Picomatch + Bash cases)"; desc = "Vetores do Picomatch (casos de divergencia). Casos Bash (extglob/globstar) documentados."; priority = 1; labels = "area/fs,type/story,prio/p1" },
    
    # Epic 6.2 - Benchmarks
    @{ title = "6.2.1 - Benchmark de matching"; desc = "10k/100k/200k paths; patterns simples e extglob. Comparar engines."; priority = 2; labels = "area/fs,risk/perf,type/story,prio/p2" },
    @{ title = "6.2.2 - Benchmark de traversal"; desc = "Usando ignore WalkBuilder (com e sem gitignore)."; priority = 2; labels = "area/fs,risk/perf,type/story,prio/p2" },
    
    # Epic 6.3 - Anti pattern bombs
    @{ title = "6.3.1 - Protecoes anti pattern bombs"; desc = "Limites e erros amigaveis + logs."; priority = 1; labels = "area/fs,risk/perf,risk/security,type/story,prio/p1" },
    
    # ============================================
    # INITIATIVE 7 - DOCS & DX
    # ============================================
    
    @{ title = "7.1.1 - Pagina Globs no Garra"; desc = "Exemplos copiaveis: **/*.rs, **/*.{ts,tsx}, excluir target/. Diferenca entre * e **."; priority = 2; labels = "area/docs,type/story,prio/p2" },
    @{ title = "7.1.2 - Como debugar match/ignore"; desc = "Ferramentas de diagnostico."; priority = 2; labels = "area/docs,type/story,prio/p2" },
    @{ title = "7.1.3 - Migration guide"; desc = "Documentar mudancas de matcher."; priority = 2; labels = "area/docs,type/story,prio/p2" }
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
                stateId = $backlogState.id
                title = $item.title
                description = $item.desc
                priority = $item.priority
            }
        }
    } | ConvertTo-Json -Depth 5
    
    try {
        $resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $mutation -ErrorAction Stop
        Write-Host "  Response: $($resp | ConvertTo-Json -Compress)" -ForegroundColor DarkGray
        if ($resp.data.issueCreate.success) {
            Write-Host "  OK: $($resp.data.issueCreate.issue.identifier)" -ForegroundColor Green
            $created++
        } else {
            Write-Host "  FALHA: $($item.title)" -ForegroundColor Red
            $failed++
        }
    } catch {
        Write-Host "  Exception: $($_.Exception.Message)" -ForegroundColor Yellow
        $failed++
    }
}

Write-Host ""
Write-Host "=== Resultado ===" -ForegroundColor Green
Write-Host "Criadas: $created" -ForegroundColor Green
Write-Host "Falhas: $failed" -ForegroundColor Red
Write-Host ""
Write-Host "Roadmap Glob & Ignore Engine v2 (Telegram-first) criado com sucesso!" -ForegroundColor Green
