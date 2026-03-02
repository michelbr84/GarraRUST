# Get all issues from Modo System project with pagination
$apiKey = "REDACTED_LINEAR_API_KEY"

$query = @"
query {
  project(id: "143fc1ac-1c39-4ab5-bc29-0105af47e210") {
    name
    issues(first: 100) {
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
    query = $query
} | ConvertTo-Json

$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
    -Method POST `
    -Headers @{
        "Authorization" = $apiKey
        "Content-Type" = "application/json"
    } `
    -Body $body

$issues = $response.data.project.issues.nodes

Write-Host "Found $($issues.Count) issues:"
foreach ($issue in $issues) {
    Write-Host "$($issue.identifier): $($issue.title) - [$($issue.state.name)]"
}
