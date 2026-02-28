# Mark GAR-180 as Done in Linear
# Uses Linear GraphQL API

$headers = @{
    "Authorization" = "$env:LINEAR_API_KEY"
    "Content-Type" = "application/json"
}

# First, get issue details using the issue ID directly
$query = @"
query {
  issue(id: "GAR-180") {
    id
    identifier
    state {
      name
      id
    }
  }
}
"@

$body = @{
    "query" = $query
} | ConvertTo-Json

Write-Host "Fetching GAR-180 details..."
$response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $body
$issue = $response.data.issue

if ($null -eq $issue) {
    Write-Host "Issue not found. Trying alternative query..."
    # Try fetching by identifier
    $altQuery = @"
query {
  issues(filter: { identifier: { eq: "GAR-180" } }) {
    nodes {
      id
      identifier
      state {
        name
        id
      }
    }
  }
}
"@
    $altBody = @{
        "query" = $altQuery
    } | ConvertTo-Json
    
    $altResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $altBody
    $issue = $altResponse.data.issues.nodes[0]
}

if ($null -eq $issue) {
    Write-Host "ERROR: Could not find issue GAR-180"
    exit 1
}

Write-Host "Issue ID: $($issue.id), Identifier: $($issue.identifier), State: $($issue.state.name)"

# Get the state ID for "Done"
$statesQuery = @"
query {
  workflowStates(first: 20) {
    nodes {
      id
      name
    }
  }
}
"@

$statesBody = @{
    "query" = $statesQuery
} | ConvertTo-Json

$statesResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $statesBody
$doneState = $statesResponse.data.workflowStates.nodes | Where-Object { $_.name -eq "Done" } | Select-Object -First 1
Write-Host "Done state ID: $($doneState.id)"

# Now update the issue - use correct mutation with id as direct argument
$updateQuery = @"
mutation {
  issueUpdate(id: "$($issue.id)", input: {
    stateId: "$($doneState.id)"
  }) {
    success
  }
}
"@

$updateBody = @{
    "query" = $updateQuery
} | ConvertTo-Json

$updateResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method POST -Headers $headers -Body $updateBody
Write-Host "Update response:"
$updateResponse | ConvertTo-Json -Depth 10

if ($updateResponse.data.issueUpdate.success) {
    Write-Host "GAR-180 marked as Done successfully!"
} else {
    Write-Host "Failed to update issue"
}
