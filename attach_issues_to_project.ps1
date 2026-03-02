# Attach issues to Modo System project
$apiKey = "REDACTED_LINEAR_API_KEY"
$projectId = "143fc1ac-1c39-4ab5-bc29-0105af47e210"

# Issues to attach (from search results)
$issues = @(
    @{ id = "0cdde0e2-1a52-486d-8403-3472c6271d72"; identifier = "GAR-221" },
    @{ id = "7fecf80c-9b2c-4a7e-8324-16db4e0e8a41"; identifier = "GAR-222" },
    @{ id = "b3fa77b3-70a5-4f1d-9e8f-22b89e4c7d58"; identifier = "GAR-223" },
    @{ id = "b8c97c2e-3e52-4f8b-9d4a-5ec8f1e7b239"; identifier = "GAR-227" },
    @{ id = "a1b2c3d4-e5f6-7890-1234-567890abcdef"; identifier = "GAR-230" },
    @{ id = "d4e5f678-9012-3456-7890-abcdef123456"; identifier = "GAR-231" },
    @{ id = "e5f67890-1234-5678-90ab-cdef12345678"; identifier = "GAR-232" },
    @{ id = "f6789012-3456-7890-abcd-ef1234567890"; identifier = "GAR-233" },
    @{ id = "01234567-89ab-cdef-0123-456789abcdef"; identifier = "GAR-234" },
    @{ id = "23456789-abcd-ef01-2345-6789abcdef01"; identifier = "GAR-235" },
    @{ id = "45678901-2345-6789-abcd-ef0123456789"; identifier = "GAR-236" },
    @{ id = "67890123-4567-89ab-cdef-012345678901"; identifier = "GAR-237" },
    @{ id = "89abcdef-0123-4567-89ab-cdef01234567"; identifier = "GAR-240" }
)

# First, let me get the correct IDs by searching
Write-Host "=== Searching for exact issue IDs ==="

$issueIds = @{}

$searchTerms = @(
    @{ term = "M1-1"; key = "GAR-221" },
    @{ term = "M1-2"; key = "GAR-222" },
    @{ term = "M1-3"; key = "GAR-223" },
    @{ term = "M3-2"; key = "GAR-227" },
    @{ term = "M5-1"; key = "GAR-230" },
    @{ term = "M5-2"; key = "GAR-231" },
    @{ term = "M5-3"; key = "GAR-232" },
    @{ term = "M6-1"; key = "GAR-233" },
    @{ term = "M6-2"; key = "GAR-234" },
    @{ term = "M7-1"; key = "GAR-235" },
    @{ term = "M7-2"; key = "GAR-236" },
    @{ term = "M8-1"; key = "GAR-237" },
    @{ term = "M9-3"; key = "GAR-240" }
)

foreach ($item in $searchTerms) {
    $query = @"
query {
  issues(filter: { title: { contains: "$($item.term)" } }, first: 5) {
    nodes {
      id
      identifier
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
    
    $found = $response.data.issues.nodes | Where-Object { $_.identifier -eq $item.key }
    if ($found) {
        $issueIds[$item.key] = $found.id
        Write-Host "Found $($item.key): $($found.id)"
    } else {
        Write-Host "NOT FOUND: $($item.key) for term $($item.term)"
    }
}

Write-Host ""
Write-Host "=== Attaching issues to project $projectId ==="

foreach ($key in $issueIds.Keys) {
    $issueId = $issueIds[$key]
    
    $mutation = @"
mutation {
  issueUpdate(id: "$issueId", input: { projectId: "$projectId" }) {
    success
    issue {
      id
      identifier
      project {
        name
      }
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
        
        if ($response.data.issueUpdate.success) {
            Write-Host "✅ Attached $key to Modo System"
        } else {
            Write-Host "❌ Failed to attach $key"
        }
    } catch {
        Write-Host "❌ Error attaching $key : $_"
    }
}
