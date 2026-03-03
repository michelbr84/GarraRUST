$apiKey = "REDACTED_LINEAR_API_KEY"
$doneStateId = "00f4eaf8-4f6f-4dbf-9527-f9c448138f6d"

$issues = @(
    @{ id = "2be184bf-2a10-4a39-8c28-b8893e3df661"; name = "GAR-211" },
    @{ id = "67eba44f-e91c-4823-86de-580c6511905c"; name = "GAR-210" }
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
    if ($resp.data.issueUpdate.success) { Write-Host "Closed $($issue.name) as Done" }
    else { Write-Host "Failed $($issue.name): $($resp | ConvertTo-Json)" }
}
