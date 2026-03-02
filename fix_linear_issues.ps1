# Check GAR-219 state and use it as reference
$apiKey = "REDACTED_LINEAR_API_KEY"

# First, get the issue details to find the correct state ID
$query = @"
query {
  issue(id: "65813b92-e41d-40d4-b017-cef30906341d") {
    id
    identifier
    state {
      id
      name
    }
  }
}
"@

$body = @{ query = $query } | ConvertTo-Json
$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
    -Method POST `
    -Headers @{
        "Authorization" = $apiKey
        "Content-Type" = "application/json"
    } `
    -Body $body

$doneStateId = $response.data.issue.state.id
Write-Host "GAR-219 Done state ID: $doneStateId"

# Now use this state ID to update GAR-240 and GAR-271
$issueIds = @{
    "GAR-240" = "4bf121d7-23be-49ba-9df2-61737cf57ed9"
    "GAR-271" = "cc87cdf3-eeff-4494-8264-ecd1499ec018"
}

# Update GAR-240 to Done
Write-Host ""
Write-Host "=== Updating GAR-240 to Done ==="
$mutation = @"
mutation {
  issueUpdate(id: "$($issueIds['GAR-240'])", input: { stateId: "$doneStateId" }) {
    success
    issue {
      id
      identifier
      state { name }
    }
  }
}
"@

$body = @{ query = $mutation } | ConvertTo-Json
$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
    -Method POST `
    -Headers @{
        "Authorization" = $apiKey
        "Content-Type" = "application/json"
    } `
    -Body $body

if ($response.data.issueUpdate.success) {
    Write-Host "✅ GAR-240 marked as Done"
} else {
    Write-Host "❌ Failed: $($response.errors | ConvertTo-Json)"
}

# Update GAR-271 to Done
Write-Host ""
Write-Host "=== Updating GAR-271 (hotfix) to Done ==="
$mutation = @"
mutation {
  issueUpdate(id: "$($issueIds['GAR-271'])", input: { stateId: "$doneStateId" }) {
    success
    issue {
      id
      identifier
      state { name }
    }
  }
}
"@

$body = @{ query = $mutation } | ConvertTo-Json
$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
    -Method POST `
    -Headers @{
        "Authorization" = $apiKey
        "Content-Type" = "application/json"
    } `
    -Body $body

if ($response.data.issueUpdate.success) {
    Write-Host "✅ GAR-271 (hotfix) marked as Done"
} else {
    Write-Host "❌ Failed: $($response.errors | ConvertTo-Json)"
}

# Now try creating blockers with correct format
Write-Host ""
Write-Host "=== Creating blockers ==="

$blockers = @(
    @{ blockerId = "1cd763d8-56e9-4010-99d9-d54eb9be1293"; blockedId = "f12ca57e-c9f3-410b-8060-1744680854c1"; label = "GAR-226 → GAR-227" }
    @{ blockerId = "70ebd1c3-2bff-44d3-be74-d49994622eae"; blockedId = "261e3037-e347-403f-9008-8893ca3e6195"; label = "GAR-223 → GAR-230" }
    @{ blockerId = "261e3037-e347-403f-9008-8893ca3e6195"; blockedId = "45ac2276-d7b8-427b-815c-522f2b73f58e"; label = "GAR-230 → GAR-231" }
    @{ blockerId = "45ac2276-d7b8-427b-815c-522f2b73f58e"; blockedId = "5e40cf36-21d4-479f-8d18-4fcf8f8c02b6"; label = "GAR-231 → GAR-232" }
    @{ blockerId = "bf74598c-77a1-4cd4-b3c1-9aaef5a2763d"; blockedId = "2dc79d17-580f-4abf-a449-8b002acd5f0d"; label = "GAR-233 → GAR-234" }
    @{ blockerId = "a87ce38d-05b5-4957-b7d0-f24b17f3fcc9"; blockedId = "0954588f-95fd-44a9-96e2-53efb0099d20"; label = "GAR-224 → GAR-235" }
)

foreach ($block in $blockers) {
    $mutation = @"
mutation {
  issueRelationCreate(input: { issueId: "$($block.blockerId)", relatedIssueId: "$($block.blockedId)", type: "blocks" }) {
    success
    issueRelation {
      id
      type
    }
  }
}
"@
    
    $body = @{ query = $mutation } | ConvertTo-Json
    
    try {
        $response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" `
            -Method POST `
            -Headers @{
                "Authorization" = $apiKey
                "Content-Type" = "application/json"
            } `
            -Body $body
        
        if ($response.data.issueRelationCreate.success) {
            Write-Host "✅ $($block.label)"
        } else {
            Write-Host "❌ $($block.label): $($response.errors | ConvertTo-Json)"
        }
    } catch {
        Write-Host "❌ Error: $($block.label) - $_"
    }
}
