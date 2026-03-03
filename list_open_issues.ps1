$apiKey = "REDACTED_LINEAR_API_KEY"

$query = @"
{
  issues(
    filter: {
      team: { key: { eq: "GAR" } }
      state: { type: { in: ["unstarted", "started", "backlog"] } }
    }
    orderBy: updatedAt
    first: 100
  ) {
    nodes {
      number
      title
      priority
      state { name type }
    }
  }
}
"@

$body = @{ query = $query } | ConvertTo-Json -Compress
$resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST `
    -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
    -Body $body

$issues = $resp.data.issues.nodes | Sort-Object { $_.priority }, { $_.number }
$issues | ForEach-Object {
    $pLabel = switch ($_.priority) {
        0 { "No" } 1 { "Urgent" } 2 { "High" } 3 { "Medium" } 4 { "Low" } default { "?" }
    }
    Write-Host ("GAR-{0,-4} [{1,-8}] [{2,-12}] {3}" -f $_.number, $pLabel, $_.state.name, $_.title)
}
Write-Host "`nTotal: $($issues.Count)"
