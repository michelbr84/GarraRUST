# PowerShell script para criar issues do OpenRouter Identity no Linear
# Uso: $env:LINEAR_API_KEY="your_api_key"; .\create_openrouter_issues.ps1

param(
    [string]$ApiKey = $env:LINEAR_API_KEY
)

if ([string]::IsNullOrEmpty($ApiKey)) {
    Write-Host "Erro: Defina a variavel LINEAR_API_KEY" -ForegroundColor Red
    Write-Host "Obtenha em: https://linear.app/settings/api" -ForegroundColor Yellow
    Write-Host "Exemplo: `$env:LINEAR_API_KEY='lin_api_xxx'; .\create_openrouter_issues.ps1" -ForegroundColor Cyan
    exit 1
}

Write-Host "Criando issues do OpenRouter Identity no Linear..." -ForegroundColor Green

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

# Buscar estados disponiveis
$stateQuery = @{ query = "query { team(id: `"$($team.id)`") { states { nodes { id name } } } }" } | ConvertTo-Json
$stateResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $stateQuery -ErrorAction Stop
$states = $stateResponse.data.team.states.nodes

# Prioridade 1 = Urgent, 2 = High, 3 = Medium, 4 = Low
$urgentState = $states | Where-Object { $_.name -eq "Backlog" } | Select-Object -First 1
if (-not $urgentState) {
    $urgentState = $states | Select-Object -First 1
}

Write-Host "Usando estado: $($urgentState.name) ($($urgentState.id))" -ForegroundColor Green

# Definir issues do OpenRouter Identity
$roadmap = @(
    # Epico 1 - Correcao Urgente
    @{
        id = "GAR-201"
        title = "[OpenRouter] Corrigir Referer e adicionar X-Title"
        desc = "## Problema

O OpenRouter mostra 'Unknown' porque os headers estao incorretos:

- `Referer: https://garraia.dev` (errado - deveria ser `garraia.org`)
- `X-Title: GarraIA` (faltando)

## Solucao

Alterar o codigo em `crates/garraia-agents/src/openai.rs` (linhas 97, 348, 403):

1. Mudar `Referer` de `https://garraia.dev` para `https://garraia.org`
2. Adicionar header `X-Title: GarraIA`

## Localizacao

- Arquivo: `crates/garraia-agents/src/openai.rs`
- Funcao: `list_models()` (linha 97)
- Funcao: `complete()` (linha 348)
- Funcao: `stream_complete()` (linha 403)

## Criterios de Aceite

- [ ] Header Referer: `https://garraia.org` enviado em todas as requisicoes
- [ ] Header X-Title: `GarraIA` enviado em todas as requisicoes
- [ ] Build passa com `cargo build`
- [ ] Testes passam com `cargo test -p garraia-agents`

**Dependencias:** Nenhuma
**Prioridade:** Urgent"
        priority = 1
    }
    
    # Epico 2 - Configuracao
    @{
        id = "GAR-202"
        title = "[OpenRouter] Tornar App Identity configuravel via config.yml"
        desc = "## Problema

Os headers estao hardcoded. Precisamos permitir configuracao via YAML.

## Solucao

Adicionar suporte a configuracao em `config.yml`:

```yaml
llm:
  openrouter:
    app:
      referer: `https://garraia.org`
      title: `GarraIA`
```

Ou via campo `extra` ja existente:

```yaml
llm:
  openrouter-main:
    provider: openrouter
    extra:
      app_referer: `https://garraia.org`
      app_title: `GarraIA`
```

## Criterios de Aceite

- [ ] Se config existir, usa os valores configurados
- [ ] Se nao existir, usa defaults seguros:
  - referer: `https://garraia.org`
  - title: `GarraIA`
- [ ] Hot reload nao quebra

**Dependencias:** GAR-201
**Prioridade:** High"
        priority = 2
    }
    
    # Epico 3 - Testes
    @{
        id = "GAR-204"
        title = "[OpenRouter] Teste unitario para headers"
        desc = "## Problema

Precisamos garantir que os headers continuem sendo enviados.

## Solucao

Criar teste unitario em `crates/garraia-agents/src/openai.rs`:

```rust
#[test]
fn openrouter_headers_included() {
    let provider = OpenAiProvider::new(
        `test-key`,
        Some(`gpt-4o`.to_string()),
        Some(`https://openrouter.ai/api/v1`.to_string()),
    );
    assert!(provider.is_openrouter);
}
```

## Criterios de Aceite

- [ ] Teste falha se headers forem removidos
- [ ] Teste passa em Windows/Linux

**Dependencias:** GAR-201
**Prioridade:** High"
        priority = 2
    }
    
    @{
        id = "GAR-205"
        title = "[OpenRouter] Validacao manual no dashboard"
        desc = "## Problema

Validar que o OpenRouter reconhece o app.

## Solucao

Roteiro de validacao manual:

1. Enviar 1 mensagem via qualquer canal (Telegram, Discord, etc.)
2. Acessar OpenRouter Dashboard > Usage
3. Verificar que a coluna App mostra GarraIA (nao Unknown)
4. Capturar evidencia (screenshot)

## Criterios de Aceite

- [ ] Evidencia postada no Linear como comentario

**Dependencias:** GAR-201
**Prioridade:** High"
        priority = 2
    }
    
    # Epico 4 - Observabilidade
    @{
        id = "GAR-206"
        title = "[OpenRouter] Log de debug para app identity"
        desc = "## Problema

Preciso saber se os headers estao sendo aplicados.

## Solucao

Adicionar log em nivel debug:

```
DEBUG: OpenRouter headers: HTTP-Referer=https://garraia.org, X-Title=GarraIA
```

## Criterios de Aceite

- [ ] Nao loga API keys
- [ ] So aparece em nivel debug/trace

**Dependencias:** GAR-201
**Prioridade:** Medium"
        priority = 3
    }
    
    # Epico 5 - Deploy
    @{
        id = "GAR-207"
        title = "[OpenRouter] Release e verificacao pos-deploy"
        desc = "## Problema

Garantir que a correcao chega em producao.

## Solucao

1. Criar tag/release (ex: v.x.x.x)
2. Fazer deploy do binario atualizado
3. Verificar no OpenRouter que App = GarraIA

## Criterios de Aceite

- [ ] Tag/release criado no GitHub
- [ ] Binario atualizado no servidor
- [ ] OpenRouter mostra App = GarraIA

**Dependencias:** GAR-205
**Prioridade:** High"
        priority = 2
    }
    
    # Epico 6 - Documentacao
    @{
        id = "GAR-208"
        title = "[OpenRouter] Documentar App Identity no README"
        desc = "## Problema

Documentar o que foi implementado.

## Solucao

Atualizar documentacao explicando:

- O que sao os headers e por que sao necessarios
- Por que Unknown aparece
- Como alterar via config
- Como validar

**Dependencias:** GAR-207
**Prioridade:** Medium"
        priority = 3
    }
    
    # Opcional - Env vars
    @{
        id = "GAR-203"
        title = "[OpenRouter] Suporte a variaveis de ambiente (opcional)"
        desc = "## Problema

Permitir override por env vars para CI/containers.

## Solucao

Vars sugeridas:
- `GARRA_OPENROUTER_REFERER`
- `GARRA_OPENROUTER_TITLE`

Ordem de precedencia:
1. Variavel de ambiente (alta prioridade)
2. Configuracao YAML
3. Defaults (baixa prioridade)

## Criterios de Aceite

- [ ] Env override funciona sem alterar config.yml
- [ ] Valores sao validados e sanitizados

**Dependencias:** GAR-202
**Prioridade:** Low"
        priority = 4
    }
)

# Criar issues
$created = 0
$failed = 0

foreach ($item in $roadmap) {
    Write-Host "Criando $($item.id): $($item.title)..." -ForegroundColor Cyan
    
    $mutation = @{
        query = "mutation CreateIssue(`$issue: IssueCreateInput!) { issueCreate(input: `$issue) { success issue { id identifier } } }"
        variables = @{
            issue = @{
                teamId = $team.id
                stateId = $urgentState.id
                title = $item.title
                description = $item.desc
                priority = $item.priority
            }
        }
    } | ConvertTo-Json -Depth 10
    
    try {
        $resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $mutation -ErrorAction Stop
        if ($resp.data.issueCreate.success) {
            Write-Host "  $($item.id) -> OK" -ForegroundColor Green
            $created++
        } else {
            Write-Host "  $($item.id) -> FALHA: $($resp | ConvertTo-Json -Compress)" -ForegroundColor Red
            $failed++
        }
    } catch {
        $errMsg = $_.Exception.Message
        if ($errMsg -like "*duplicate*") {
            Write-Host "  $($item.id) -> JA EXISTE (pulando)" -ForegroundColor Yellow
        } else {
            try {
                $errResp = [System.Text.Encoding]::UTF8.GetString($_.Exception.Response.GetResponseStream().ReadToEnd())
                Write-Host "  $($item.id) -> ERRO: $errMsg | Response: $errResp" -ForegroundColor Red
            } catch {
                Write-Host "  $($item.id) -> ERRO: $errMsg" -ForegroundColor Red
            }
        }
        $failed++
    }
}

Write-Host ""
Write-Host "=== Resultado ===" -ForegroundColor Green
Write-Host "Criadas: $created" -ForegroundColor Green
Write-Host "Falhas: $failed" -ForegroundColor Red
Write-Host ""
Write-Host "Roadmap OpenRouter criado com sucesso!" -ForegroundColor Green
