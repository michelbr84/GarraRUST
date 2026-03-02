# Final fixes - Add missing blocker and check milestones
$apiKey = "REDACTED_LINEAR_API_KEY"

Write-Host "=== Step 1: Creating missing blocker GAR-232 -> GAR-233 ==="

$mutation = @"
mutation {
  issueRelationCreate(input: { issueId: "5e40cf36-21d4-479f-8d18-4fcf8f8c02b6", relatedIssueId: "bf74598c-77a1-4cd4-b3c1-9aaef5a2763d", type: blocks }) {
    success
    issueRelation {
      id
      type
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

if ($response.data.issueRelationCreate.success) {
    Write-Host "SUCCESS: GAR-232 -> GAR-233 blocker created"
} else {
    Write-Host "FAILED: $($response.errors | ConvertTo-Json)"
}

Write-Host ""
Write-Host "=== Step 2: Getting available milestones ==="

$query = @"
query {
  project(id: "143fc1ac-1c39-4ab5-bc29-0105af47e210") {
    milestones(first: 10) {
      nodes {
        id
        name
        state
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

$milestones = $response.data.project.milestones.nodes
Write-Host "Available milestones:"
if ($milestones.Count -gt 0) {
    foreach ($m in $milestones) {
        Write-Host "  - $($m.name) (id: $($m.id), state: $($m.state))"
    }
} else {
    Write-Host "  No milestones found"
}

Write-Host ""
Write-Host "=== Step 3: Listing all blockers ==="

$query = @"
query {
  project(id: "143fc1ac-1c39-4ab5-bc29-0105af47e210") {
    name
    issues(first: 30) {
      nodes {
        id
        identifier
        title
        blockedByIssues(first: 5) {
          nodes {
            id
            identifier
            title
          }
        }
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

Write-Host "Blocker relationships in project:"
foreach ($issue in $response.data.project.issues.nodes) {
    $blockers = $issue.blockedByIssues.nodes
    if ($blockers.Count -gt 0) {
        $blockerIds = ($blockers | ForEach-Object { $_.identifier }) -join ", "
        Write-Host "  $($issue.identifier) <- blocked by [$blockerIds]"
    }
}
