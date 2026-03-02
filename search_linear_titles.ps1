# Search for issues by title in Linear - Multiple searches
$apiKey = "REDACTED_LINEAR_API_KEY"

$searchTerms = @(
    "M1-1",
    "M1-2",
    "M1-3",
    "M3-2",
    "M5-1",
    "M5-2",
    "M5-3",
    "M6-1",
    "M6-2",
    "M7-1",
    "M7-2",
    "M8-1",
    "M9-3"
)

foreach ($term in $searchTerms) {
    $query = @"
query {
  issues(filter: { title: { contains: "$term" } }, first: 10) {
    nodes {
      id
      identifier
      title
      project {
        name
      }
      state {
        name
      }
    }
  }
}
"@
    
    $body = @{
        query = $query
    } | ConvertTo-Json
    
    $response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
        -Method POST `
        -Headers @{
            "Authorization" = $apiKey
            "Content-Type" = "application/json"
        } `
        -Body $body
    
    $issues = $response.data.issues.nodes
    Write-Host "=== Searching for: $term ==="
    if ($issues.Count -gt 0) {
        foreach ($issue in $issues) {
            $projName = if ($issue.project) { $issue.project.name } else { "NULL" }
            Write-Host "  $($issue.identifier): $($issue.title) | Project: $projName | State: $($issue.state.name)"
        }
    } else {
        Write-Host "  NOT FOUND"
    }
    Write-Host ""
}
