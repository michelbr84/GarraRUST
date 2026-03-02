#!/usr/bin/env pwsh

# Create urgent issue for router fix in Linear

$ErrorActionPreference = "Stop"

# Load API key from .env
$envPath = Join-Path $PSScriptRoot ".env"
if (Test-Path $envPath) {
    Get-Content $envPath | ForEach-Object {
        if ($_ -match "^LINEAR_API_KEY=(.+)$") {
            $env:LINEAR_API_KEY = $matches[1].Trim()
        }
    }
}

if (-not $env:LINEAR_API_KEY) {
    Write-Host "LINEAR_API_KEY not set." -ForegroundColor Red
    exit 1
}

$headers = @{
    "Authorization" = $env:LINEAR_API_KEY
    "Content-Type" = "application/json"
}

# First, get the team ID for GAR
$teamQuery = '{"query":"{ teams(first: 10) { nodes { id key name } } }"}'
$teamResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method Post -Headers $headers -Body $teamQuery

$garTeam = $teamResponse.data.teams.nodes | Where-Object { $_.key -eq "GAR" }
if (-not $garTeam) {
    Write-Host "Could not find GAR team" -ForegroundColor Red
    exit 1
}

$teamId = $garTeam.id
Write-Host "Found GAR team: $teamId"

$title = "[URGENT] Fix gateway startup panic: axum route segments"
$description = "O gateway faz panic fatal na inicialização. Erro: Path segments must not start with ':'. Causa: rota /:id deveria ser /{id}. Correção aplicada em router.rs linha 90."
$labels = "bug,urgent"

# Build JSON manually
$json = @"
{
  "query": "mutation { issueCreate(input: { teamId: `"$teamId`", title: `"$title`", description: `"$description`", labels: [`"$labels`"] }) { success issue { id identifier title } } }"
}
"@

try {
    $result = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method Post -Headers $headers -Body $json
    
    if ($result.data.issueCreate.success) {
        $issue = $result.data.issueCreate.issue
        Write-Host "SUCCESS: Created issue [$($issue.identifier)] - $($issue.title)" -ForegroundColor Green
        Write-Host "URL: https://linear.app/chatgpt25/issue/$($issue.identifier)" -ForegroundColor Cyan
    } else {
        Write-Host "Failed to create issue" -ForegroundColor Red
    }
} catch {
    Write-Host "Error: $_" -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
}
