# PSScriptAnalyzer configuration for install.ps1.
#
# Rules are excluded here rather than through inline SuppressMessage attributes
# so the justification lives in one reviewable place, and so the analyzer stays
# a hard gate (`Invoke-ScriptAnalyzer` output non-empty => CI fails) instead of
# a list of warnings everyone learns to scroll past.
@{
    ExcludeRules = @(
        # An installer's console output IS its user interface. Write-Output
        # would put the progress chatter on the success stream, where
        # `irm ... | iex` callers and the Pester suites would capture it as
        # return values; Write-Information is invisible by default in
        # Windows PowerShell 5.1, which is the primary target. Write-Host is
        # the correct cmdlet for this specific job.
        'PSAvoidUsingWriteHost'
    )
}
