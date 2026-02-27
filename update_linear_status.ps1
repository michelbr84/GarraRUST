#!/usr/bin/env pwsh

# Update Linear issues status based on code analysis
# Usage: $env:LINEAR_API_KEY="lin_api_xxxx"; .\update_linear_status.ps1

$ErrorActionPreference = "Stop"

if (-not $env:LINEAR_API_KEY) {
    Write-Host "❌ LINEAR_API_KEY not set." -ForegroundColor Red
    exit 1
}

$headers = @{
    "Authorization" = $env:LINEAR_API_KEY
    "Content-Type" = "application/json"
}

# Issues that are IMPLEMENTED in code but not marked as Done in Linear
$implementedIssues = @(
    @{
        id = "GAR-180"
        title = "Voice Handler Real"
        evidence = "✅ IMPLEMENTADO em bootstrap.rs:1200-1352
- build_telegram_voice_handler() processa voz completa
- STT (Whisper) + LLM + TTS (Chatterbox)
- voice_handler.rs tem synthesize() e transcribe()
- Endpoint /api/tts e /api/stt funcionais"
        file = "crates/garraia-gateway/src/bootstrap.rs"
        lines = "1200-1352"
    },
    @{
        id = "GAR-181"
        title = "Voice E2E Telegram"
        evidence = "✅ IMPLEMENTADO em bootstrap.rs:1200-1352
- Pipeline completo: Telegram voice -> STT -> LLM -> TTS -> Telegram voice
- Fallback para texto se TTS falhar
- Allowlist verificado para usuários de voz"
        file = "crates/garraia-gateway/src/bootstrap.rs"
        lines = "1200-1352"
    }
)

# Get "Done" state ID
Write-Host "Fetching workflow states..." -ForegroundColor Cyan
$teamQuery = @{ query = "query { teams { nodes { id name key states { nodes { id name } } } } }" } | ConvertTo-Json
$teamResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $teamQuery -ErrorAction Stop
$team = $teamResponse.data.teams.nodes | Where-Object { $_.key -eq "GAR" }

if (-not $team) {
    Write-Host "❌ Team GAR not found" -ForegroundColor Red
    exit 1
}

$doneState = $team.states.nodes | Where-Object { $_.name -eq "Done" } | Select-Object -First 1

if (-not $doneState) {
    Write-Host "❌ 'Done' state not found" -ForegroundColor Red
    exit 1
}

Write-Host "Team: $($team.name) | Done State: $($doneState.id)" -ForegroundColor Green

# Update each issue
foreach ($issue in $implementedIssues) {
    Write-Host "`nUpdating $($issue.id) - $($issue.title)..." -ForegroundColor Cyan
    
    # First, find the issue by number (not identifier)
    $issueNumber = $issue.id -replace "GAR-", ""
    
    $searchQuery = @"
{
  "query": "query { issue(id: `"$issueNumber`") { id identifier title state { name } } }"
}
"@
    
    try {
        $searchResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $searchQuery -ErrorAction Stop
        $foundIssue = $searchResponse.data.issue
        
        if (-not $foundIssue) {
            # Try using number filter
            $searchQuery2 = @"
{
  "query": "query { issues(filter: { number: { eq: $issueNumber } }) { nodes { id identifier title state { name } } } }"
}
"@
            $searchResponse2 = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $searchQuery2 -ErrorAction Stop
            $foundIssue = $searchResponse2.data.issues.nodes | Select-Object -First 1
        }
        
        if (-not $foundIssue) {
            Write-Host "  ⚠️  Issue $($issue.id) not found in Linear" -ForegroundColor Yellow
            continue
        }
        
        Write-Host "  Found: $($foundIssue.title) (State: $($foundIssue.state.name))" -ForegroundColor Gray
        $issueId = $foundIssue.id
        
        # Build comment with evidence
        $commentBody = "

## Verificacao de Implementacao

**Status:** JA IMPLEMENTADO NO CODIGO

**Arquivo:** $($issue.file):$($issue.lines)

**Evidencia:**
$($issue.evidence)

---
*Atualizado via script de analise em $(Get-Date -Format 'yyyy-MM-dd HH:mm')*"
        
        # Add comment
        $commentMutation = @"
{
  "query": "mutation { issueCommentCreate(input: { issueId: `"$issueId`", body: `"$($commentBody -replace '"', '\"' -replace '`n', '\n' -replace '\r', '')`" }) { success } }"
}
"@
        
        try {
            $commentResp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $commentMutation -ErrorAction Stop
            if ($commentResp.data.issueCommentCreate.success) {
                Write-Host "  ✅ Comment added" -ForegroundColor Green
            }
        } catch {
            Write-Host "  ⚠️  Comment failed (may already exist): $($_.Exception.Message)" -ForegroundColor Yellow
        }
        
        # Update state to Done
        $updateMutation = @"
{
  "query": "mutation { issueUpdate(id: `"$issueId`", input: { stateId: `"$($doneState.id)`" }) { success } }"
}
"@
        
        $updateResp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $updateMutation -ErrorAction Stop
        
        if ($updateResp.data.issueUpdate.success) {
            Write-Host "  ✅ State updated to Done" -ForegroundColor Green
        } else {
            Write-Host "  ❌ Failed to update state" -ForegroundColor Red
        }
        
    } catch {
        Write-Host "  ❌ Error: $($_.Exception.Message)" -ForegroundColor Red
    }
}

Write-Host "`n=== Done! ===" -ForegroundColor Green
