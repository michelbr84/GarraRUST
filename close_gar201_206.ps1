$apiKey = "REDACTED_LINEAR_API_KEY"
$doneStateId = "00f4eaf8-4f6f-4dbf-9527-f9c448138f6d"

$issues = @(
    @{ id = "d7e3bf7a-460b-4172-adb1-9b15cf7490c1"; name = "GAR-201" },
    @{ id = "4ddda5bb-ba34-40df-bd24-bf658a73083d"; name = "GAR-206" }
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
