# Get all issues from Modo System project with better pagination
$apiKey = "REDACTED_LINEAR_API_KEY"

# First query - get first 50
$query1 = @"
query {
  project(id: "143fc1ac-1c39-4ab5-bc29-0105af47e210") {
    name
    issues(first: 50, after: null) {
      nodes {
        id
        identifier
        title
        state {
          name
        }
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
"@

$body = @{
    query = $query1
} | ConvertTo-Json

$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
    -Method POST `
    -Headers @{
        "Authorization" = $apiKey
        "Content-Type" = "application/json"
    } `
    -Body $body

$issues = $response.data.project.issues.nodes

Write-Host "=== Page 1: Found $($issues.Count) issues ==="
foreach ($issue in $issues) {
    Write-Host "$($issue.identifier): $($issue.state.name)"
}

# Check if there are more pages
$pageInfo = $response.data.project.issues.pageInfo
Write-Host "Has next page: $($pageInfo.hasNextPage)"
Write-Host "End cursor: $($pageInfo.endCursor)"

# If there's more, get page 2
if ($pageInfo.hasNextPage) {
    $cursor = $pageInfo.endCursor
    
    $query2 = @"
query {
  project(id: "143fc1ac-1c39-4ab5-bc29-0105af47e210") {
    name
    issues(first: 50, after: "$cursor") {
      nodes {
        id
        identifier
        title
        state {
          name
        }
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
"@
    
    $body2 = @{
        query = $query2
    } | ConvertTo-Json
    
    $response2 = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
        -Method POST `
        -Headers @{
            "Authorization" = $apiKey
            "Content-Type" = "application/json"
        } `
        -Body $body2
    
    $issues2 = $response2.data.project.issues.nodes
    Write-Host "=== Page 2: Found $($issues2.Count) issues ==="
    foreach ($issue in $issues2) {
        Write-Host "$($issue.identifier): $($issue.state.name)"
    }
}
