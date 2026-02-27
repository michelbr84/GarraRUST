# Mark GAR-180 as Done in Linear
# First, get the issue ID and the "done" state ID

$headers = @{
    "Authorization" = "Bearer $env:LINEAR_API_KEY"
    "Content-Type" = "application/json"
}

# First, get issue details to find the issueId (numeric)
$query = @"
query {
  issue(identifier: "GAR-180") {
    id
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
Write-Host "Issue ID: $($issue.id), State: $($issue.state.name)"

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
$doneState = $statesResponse.data.workflowStates.nodes | Where-Object { $_.name -eq "Done" }
Write-Host "Done state ID: $($doneState.id)"

# Now update the issue
$updateQuery = @"
mutation {
  issueUpdate(input: {
    id: "$($issue.id)"
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
