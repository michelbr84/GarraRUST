# Load from .env if exists
$envPath = ".env"
if (Test-Path $envPath) {
    Get-Content $envPath | ForEach-Object {
        if ($_ -match "^LINEAR_API_KEY=(.+)$") {
            $env:LINEAR_API_KEY = $matches[1].Trim()
        }
    }
}

$apiKey = $env:LINEAR_API_KEY
if (-not $apiKey) {
    Write-Host "LINEAR_API_KEY not found in environment or .env" -ForegroundColor Red
    exit 1
}

# Get GAR team by key
$teamQuery = @"
{
    teams(filter: { key: { eq: "GAR" } }) {
        nodes {
            id
            name
            key
        }
    }
}
"@

$teamBody = @{ query = $teamQuery } | ConvertTo-Json -Compress
$teamResponse = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method Post -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } -Body $teamBody

if ($teamResponse.data.teams.nodes.Count -eq 0) {
    Write-Host "Team GAR not found" -ForegroundColor Red
    exit 1
}

$teamId = $teamResponse.data.teams.nodes[0].id
Write-Host "Found GAR team: $teamId"

$title = "[BUG] Fix gateway startup panic: axum route segments"
$description = @"
## Description
The gateway panics on startup with the error: `Path segments must not start with ':'`

## Root Cause
The router uses old-style axum capture syntax `/:id` instead of `/{id}` which is invalid in axum v0.7+

## Fix Applied
Changed line 90 in `crates/garraia-gateway/src/router.rs`:
- BEFORE: `.route("/api/modes/custom/:id", ...)`
- AFTER: `.route("/api/modes/custom/{id}", ...)`

## Impact
- Auth tests were failing
- Gateway would not start in production

## Status
Fixed and verified - all 274+ tests pass

## Labels
- bug
- urgent
"@

$query = @"
mutation {
    issueCreate(input: {
        teamId: "$teamId",
        title: "$title",
        description: $(($description -replace '"', '\"') | ConvertTo-Json -Compress)
    }) {
        success
        issue {
            id
            identifier
        }
    }
}
"@

$json = @{ query = $query } | ConvertTo-Json -Compress
Write-Host "JSON: $json"

try {
    $body = @{ query = $query } | ConvertTo-Json -Compress
    $response = Invoke-RestMethod -Uri "https://api.linear.app/graphql" -Method Post -Headers @{ "Authorization" = $apiKey; "Content-Type" = "application/json" } -Body $body
    
    if ($response.errors) {
        Write-Host "GraphQL Error: $($response.errors[0].message)" -ForegroundColor Red
    } elseif ($response.data.issueCreate.success) {
        Write-Host "SUCCESS: Created issue $($response.data.issueCreate.issue.identifier)" -ForegroundColor Green
    } else {
        Write-Host "Failed to create issue" -ForegroundColor Red
    }
} catch {
    Write-Host "Error: $_" -ForegroundColor Red
}
