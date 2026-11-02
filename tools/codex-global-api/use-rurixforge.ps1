[CmdletBinding()]
param([switch]$OpenAI)

# Dot-source this file to refresh an existing terminal. The child scope keeps
# private values out of caller variables; process environment changes persist.
& {
    param([bool]$MapOpenAI)
    $ErrorActionPreference = 'Stop'
    $values = @{}
    foreach ($name in @('RURIXFORGE_API_KEY', 'RURIXFORGE_BASE_URL', 'RURIXFORGE_MODEL')) {
        $value = [Environment]::GetEnvironmentVariable($name, 'User')
        if ([string]::IsNullOrWhiteSpace($value)) {
            throw ('Missing user environment variable ' + $name + '. Run setup.py first.')
        }
        $values[$name] = $value
    }
    foreach ($name in $values.Keys) {
        [Environment]::SetEnvironmentVariable($name, $values[$name], 'Process')
    }
    if ($MapOpenAI) {
        [Environment]::SetEnvironmentVariable('OPENAI_API_KEY', $values.RURIXFORGE_API_KEY, 'Process')
        [Environment]::SetEnvironmentVariable('OPENAI_BASE_URL', $values.RURIXFORGE_BASE_URL, 'Process')
    }
    Write-Host ('RurixForge client environment loaded. Model: ' + $values.RURIXFORGE_MODEL)
    if ($MapOpenAI) { Write-Host 'OPENAI_API_KEY and OPENAI_BASE_URL are mapped for this process only.' }
} ([bool]$OpenAI)
