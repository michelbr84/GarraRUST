# PSScriptAnalyzer configuration for install.ps1.
#
# Rules are excluded here rather than through inline SuppressMessage attributes
# so the justification lives in one reviewable place, and so the analyzer stays
# a hard gate (`Invoke-ScriptAnalyzer` output non-empty => CI fails) instead of
# a list of warnings everyone learns to scroll past.
@{
    # `install.ps1` is pasted into Windows PowerShell 5.1, not just pwsh 7, so
    # the analyzer is told to check against 5.1 explicitly. A local `pwsh 7`
    # run cannot catch a 5.1-only break by executing -- these rules catch the
    # class statically. (They do NOT catch positional-argument arity, which is
    # why run_lint.ps1 carries its own Join-Path guard.)
    Rules = @{
        PSUseCompatibleSyntax = @{
            Enable         = $true
            TargetVersions = @('5.1', '7.0')
        }
        PSUseCompatibleCommands = @{
            Enable = $true
            # Ships with PSScriptAnalyzer under compatibility_profiles/.
            TargetProfiles = @(
                'win-8_x64_10.0.17763.0_5.1.17763.316_x64_4.0.30319.42000_framework'
            )
        }
    }

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
