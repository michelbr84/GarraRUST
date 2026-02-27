#!/usr/bin/env pwsh

# Fetch issues from Linear GAR team
# Usage: $env:LINEAR_API_KEY="lin_api_xxxx"; .\fetch_linear_issues.ps1

$ErrorActionPreference = "Stop"

if (-not $env:LINEAR_API_KEY) {
    Write-Host "❌ LINEAR_API_KEY not set. Please set it first:" -ForegroundColor Red
    Write-Host '  $env:LINEAR_API_KEY="lin_api_xxxx"' -ForegroundColor Yellow
    exit 1
}

$headers = @{
    "Authorization" = $env:LINEAR_API_KEY
    "Content-Type" = "application/json"
}

$query = @"
query {
  issues(filter: { team: { key: { eq: "GAR" } } }, first: 100) {
    nodes {
      id
      identifier
      title
      description
      priority
      state {
        name
      }
      labels {
        nodes {
          name
        }
      }
      assignee {
        name
      }
      createdAt
      updatedAt
    }
    pageInfo {
      hasNextPage
      endCursor
    }
  }
}
"@

$body = @{ query = $query } | ConvertTo-Json -Depth 10

try {
    Write-Host "🔍 Fetching GAR issues from Linear..." -ForegroundColor Cyan
    
    $response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
        -Method Post `
        -Headers $headers `
        -Body $body
    
    $issues = $response.data.issues.nodes
    
    Write-Host "`n📋 Found $($issues.Count) issues:`n" -ForegroundColor Green
    
    # Sort by priority (P0 first - lower priority number = higher priority in Linear)
    $sortedIssues = $issues | Sort-Object { $_.priority } -Descending
    
    foreach ($issue in $sortedIssues) {
        $priorityLabel = switch ($issue.priority) {
            0 { "🔴 P0 - Urgent" }
            1 { "🟠 P1 - High" }
            2 { "🟡 P2 - Medium" }
            3 { "🟢 P3 - Low" }
            4 { "⚪ P4 - Backlog" }
            default { "⚪ P$($issue.priority)" }
        }
        
        $labels = ($issue.labels.nodes | ForEach-Object { $_.name }) -join ", "
        if (-not $labels) { $labels = "none" }
        
        $state = $issue.state.name
        $assignee = if ($issue.assignee) { $issue.assignee.name } else { "Unassigned" }
        
        Write-Host "========================================" -ForegroundColor Gray
        Write-Host "[$($issue.identifier)] $priorityLabel" -ForegroundColor White
        Write-Host "Title: $($issue.title)" -ForegroundColor White
        Write-Host "State: $state | Assignee: $assignee" -ForegroundColor Gray
        Write-Host "Labels: $labels" -ForegroundColor Gray
        
        # Show description preview (first 200 chars)
        if ($issue.description) {
            $descPreview = $issue.description.Substring(0, [Math]::Min(200, $issue.description.Length))
            Write-Host "Description: $descPreview..." -ForegroundColor DarkGray
        }
        Write-Host ""
    }
    
    # Export to JSON for further processing
    $issues | ConvertTo-Json -Depth 10 | Out-File -FilePath "linear_issues.json" -Encoding UTF8
    Write-Host "💾 Exported to linear_issues.json" -ForegroundColor Green
    
} catch {
    Write-Host "❌ Error fetching issues: $_" -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
    exit 1
}
