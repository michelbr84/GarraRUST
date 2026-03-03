$apiKey = "REDACTED_LINEAR_API_KEY"
$doneStateId = "00f4eaf8-4f6f-4dbf-9527-f9c448138f6d"

# Issues already implemented: GAR-198 (schema) and GAR-203 (endpoint)
$issues = @(
    @{ id = "8b0dd673-ca04-4458-9a46-2480c608eebb"; name = "GAR-198" },
    @{ id = "668c3eb6-78d1-49bd-9c2c-190eff945d35"; name = "GAR-203" }
)

foreach ($issue in $issues) {
    $mutation = @"
mutation {
  issueUpdate(id: "$($issue.id)", input: { stateId: "$doneStateId" }) {
    success
  }
}
"@
    $body = @{ query = $mutation } | ConvertTo-Json -Compress
    $resp = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST `
        -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } `
        -Body $body
    if ($resp.data.issueUpdate.success) {
        Write-Host "Closed $($issue.name) as Done"
    } else {
        Write-Host "Failed to close $($issue.name): $($resp | ConvertTo-Json)"
    }
}
