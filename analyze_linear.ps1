$headers = @{
    "Authorization" = "REDACTED_LINEAR_API_KEY"
    "Content-Type" = "application/json"
}

$query = @"
query {
  issues(filter: { team: { key: { eq: "GAR" } }, first: 200) {
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
    }
  }
}
"@

$body = @{ query = $query } | ConvertTo-Json

$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $body
$issues = $response.data.issues.nodes

Write-Host "=== TOTAL ISSUES: $($issues.Count) ===" -ForegroundColor Cyan
Write-Host ""

# Group by state
$issues | Group-Object { $_.state.name } | ForEach-Object {
    $stateName = $_.Name
    $count = $_.Group.Count
    Write-Host "=== $stateName ($count) ===" -ForegroundColor $(if ($stateName -eq "Todo") { "Yellow" } elseif ($stateName -eq "Done") { "Green" } elseif ($stateName -eq "Canceled") { "Red" } else { "White" })
    
    $_.Group | Sort-Object { [int]($_.identifier -replace "GAR-", "") } | ForEach-Object {
        $labels = ($_.labels.nodes | ForEach-Object { $_.name }) -join ", "
        if (-not $labels) { $labels = "none" }
        Write-Host "  $($_.identifier) P$($_.priority): $($_.title)" -ForegroundColor White
    }
    Write-Host ""
}
