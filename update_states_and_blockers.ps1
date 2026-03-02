# Update states and create blockers - Fixed API calls
$apiKey = "REDACTED_LINEAR_API_KEY"

# Issue IDs (from previous search)
$issueIds = @{
    "GAR-219" = "65813b92-e41d-40d4-b017-cef30906341d"
    "GAR-220" = "e1e45143-d481-4d84-ad0b-0d7260b66064"
    "GAR-221" = "0cdde0e2-1a52-486d-8403-3472c6271d72"
    "GAR-222" = "be20203c-921b-4344-983b-b078bdb4738e"
    "GAR-223" = "70ebd1c3-2bff-44d3-be74-d49994622eae"
    "GAR-224" = "a87ce38d-05b5-4957-b7d0-f24b17f3fcc9"
    "GAR-225" = "45362714-200a-42af-b02d-04d0d936aebd"
    "GAR-226" = "1cd763d8-56e9-4010-99d9-d54eb9be1293"
    "GAR-227" = "f12ca57e-c9f3-410b-8060-1744680854c1"
    "GAR-228" = "0190ea38-8524-47f7-ae4d-58809a721761"
    "GAR-229" = "72f4f092-38e5-4d96-a170-fbb35ae9e3e0"
    "GAR-230" = "261e3037-e347-403f-9008-8893ca3e6195"
    "GAR-231" = "45ac2276-d7b8-427b-815c-522f2b73f58e"
    "GAR-232" = "5e40cf36-21d4-479f-8d18-4fcf8f8c02b6"
    "GAR-233" = "bf74598c-77a1-4cd4-b3c1-9aaef5a2763d"
    "GAR-234" = "2dc79d17-580f-4abf-a449-8b002acd5f0d"
    "GAR-235" = "0954588f-95fd-44a9-96e2-53efb0099d20"
    "GAR-236" = "f74fa4b0-ade4-463f-993e-e627e4ffa7a4"
    "GAR-237" = "cb94ebb1-7aa3-4dea-b331-314de55d9396"
    "GAR-238" = "9bed89d8-f732-4330-b27f-c1012cbf33d6"
    "GAR-239" = "31c82b70-18c3-4ed3-bafa-2a5b40518724"
    "GAR-240" = "4bf121d7-23be-49ba-9df2-61737cf57ed9"
    "GAR-271" = "cc87cdf3-eeff-4494-8264-ecd1499ec018"
}

# First, get the state ID for "Done" - using exact name from project
Write-Host "=== Getting state IDs from project ==="
$query = @"
query {
  project(id: "143fc1ac-1c39-4ab5-bc29-0105af47e210") {
    states {
      nodes {
        id
        name
      }
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

$states = $response.data.project.states.nodes
Write-Host "Available states:"
foreach ($s in $states) {
    Write-Host "  $($s.name): $($s.id)"
}

$doneStateId = ($states | Where-Object { $_.name -eq "Done" }).id

Write-Host ""
Write-Host "Using Done state ID: $doneStateId"

# Step C: Mark GAR-240 as Done
Write-Host ""
Write-Host "=== Step C: Marking GAR-240 as Done ==="
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
    Write-Host "❌ Failed to mark GAR-240 as Done: $($response.errors)"
}

# Step D: Create blockers using correct API
Write-Host ""
Write-Host "=== Step D: Creating blockers ==="

$blockers = @(
    @{ blocker = "GAR-226"; blocked = "GAR-227" }  # GAR-226 Blocks GAR-227
    @{ blocker = "GAR-223"; blocked = "GAR-230" }  # GAR-223 Blocks GAR-230
    @{ blocker = "GAR-230"; blocked = "GAR-231" }  # GAR-230 Blocks GAR-231
    @{ blocker = "GAR-231"; blocked = "GAR-232" }  # GAR-231 Blocks GAR-232
    @{ blocker = "GAR-233"; blocked = "GAR-234" }  # GAR-233 Blocks GAR-234
    @{ blocker = "GAR-224"; blocked = "GAR-235" }  # GAR-224 Blocks GAR-235
)

foreach ($block in $blockers) {
    $blockerId = $issueIds[$block.blocker]
    $blockedId = $issueIds[$block.blocked]
    
    # Using the correct input format for issueRelationCreate
    $mutation = @"
mutation {
  issueRelationCreate(input: { fromId: "$blockerId", toId: "$blockedId", relationType: "blocks" }) {
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
            Write-Host "✅ $($block.blocker) blocks $($block.blocked)"
        } else {
            Write-Host "❌ Failed: $($block.blocker) blocks $($block.blocked) - $($response.errors)"
        }
    } catch {
        Write-Host "❌ Error: $($block.blocker) blocks $($block.blocked) - $_"
    }
}

# Step E: Mark hotfix GAR-271 as Done
Write-Host ""
Write-Host "=== Step E: Marking GAR-271 (hotfix) as Done ==="
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
    Write-Host "❌ Failed to mark GAR-271 as Done: $($response.errors)"
}
