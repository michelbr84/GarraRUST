# List all projects via viewer
$apiKey = "REDACTED_LINEAR_API_KEY"

$query = @"
query {
  viewer {
    id
    name
    projects(first: 50) {
      nodes {
        id
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

$response | ConvertTo-Json -Depth 10
