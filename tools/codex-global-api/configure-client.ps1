[CmdletBinding()]
param(
    [string]$CredentialsPath = (Join-Path $env:USERPROFILE '.config\rurixforge\codex-api.json'),
    [string]$CodexConfigPath = (Join-Path $env:USERPROFILE '.codex\config.toml')
)

$ErrorActionPreference = 'Stop'

# Keep credentials out of TOML, console output, and command-line arguments.
try {
    $client = [IO.File]::ReadAllText($CredentialsPath) | ConvertFrom-Json
} catch {
    throw 'Cannot read the private RurixForge client configuration. Run setup.py first.'
}
$baseUrl = [string]$client.baseUrl
$model = [string]$client.model
if ([string]::IsNullOrWhiteSpace([string]$client.apiKey) -or
    [string]::IsNullOrWhiteSpace($baseUrl) -or
    [string]::IsNullOrWhiteSpace($model)) {
    throw 'The private client configuration must contain apiKey, baseUrl, and model.'
}
if ($baseUrl -match '[\r\n]' -or $model -match '[\r\n]') {
    throw 'The base URL and model must be single-line values.'
}
$uri = $null
if (-not [Uri]::TryCreate($baseUrl, [UriKind]::Absolute, [ref]$uri) -or
    $uri.Scheme -notin @('http', 'https') -or $uri.UserInfo -or $uri.Query -or $uri.Fragment) {
    throw 'The base URL must be an HTTP(S) URL without credentials, query, or fragment.'
}
$baseUrl = $baseUrl.TrimEnd('/')
$client = $null

function ConvertTo-TomlString([string]$Value) {
    return '"' + $Value.Replace('\', '\\').Replace('"', '\"') + '"'
}

$configPath = [IO.Path]::GetFullPath($CodexConfigPath)
$parentPath = [IO.Path]::GetDirectoryName($configPath)
$exists = [IO.File]::Exists($configPath)
$original = if ($exists) { [IO.File]::ReadAllText($configPath) } else { '' }
$originalBytes = if ($exists) { [IO.File]::ReadAllBytes($configPath) } else { [byte[]]@() }
$hasBom = $originalBytes.Length -ge 3 -and $originalBytes[0] -eq 0xef -and
    $originalBytes[1] -eq 0xbb -and $originalBytes[2] -eq 0xbf
$newline = if ($original.Contains("`r`n")) { "`r`n" } else { "`n" }
$begin = '# BEGIN RurixForge global Codex API'
$end = '# END RurixForge global Codex API'
$managedPattern = '(?ms)^' + [Regex]::Escape($begin) + '\r?\n.*?^' + [Regex]::Escape($end) + '(?:\r?\n|$)'
$matches = [Regex]::Matches($original, $managedPattern)
if ($matches.Count -gt 1 -or
    ([Regex]::Matches($original, [Regex]::Escape($begin))).Count -ne $matches.Count -or
    ([Regex]::Matches($original, [Regex]::Escape($end))).Count -ne $matches.Count) {
    throw 'The existing RurixForge managed block is ambiguous. No configuration was changed.'
}
$unmanaged = [Regex]::Replace($original, $managedPattern, '')
$sectionConflict = '(?m)^\s*\[\s*(?:model_providers|profiles)\s*\.\s*["'']?rurixforge["'']?\s*(?:\.|\])'
$inlineConflict = '(?m)^\s*["'']?rurixforge["'']?\s*='
if ($unmanaged -match $sectionConflict -or $unmanaged -match $inlineConflict) {
    throw 'An unmanaged rurixforge provider/profile already exists. No configuration was changed.'
}

$block = @(
    $begin
    '[model_providers.rurixforge]'
    'name = "RurixForge"'
    ('base_url = ' + (ConvertTo-TomlString $baseUrl))
    'env_key = "RURIXFORGE_API_KEY"'
    'wire_api = "responses"'
    'requires_openai_auth = false'
    'supports_websockets = false'
    $end
) -join $newline
$block += $newline
if ($matches.Count -eq 1) {
    $match = $matches[0]
    $updated = $original.Substring(0, $match.Index) + $block +
        $original.Substring($match.Index + $match.Length)
} else {
    $separator = if (-not $original) { '' } elseif ($original.EndsWith("`n")) { $newline } else { $newline + $newline }
    $updated = $original + $separator + $block
}

# Current Codex uses named sibling files for profiles. A legacy [profiles.*]
# table in config.toml prevents --profile from loading, so the managed global
# block above intentionally contains only the provider.
$profilePath = Join-Path $parentPath 'rurixforge.config.toml'
$profileExists = [IO.File]::Exists($profilePath)
$profileOriginal = if ($profileExists) { [IO.File]::ReadAllText($profilePath) } else { '' }
$profileBegin = '# BEGIN RurixForge Codex profile v2'
$profileEnd = '# END RurixForge Codex profile v2'
$profilePattern = '\A' + [Regex]::Escape($profileBegin) + '\r?\n[\s\S]*?\r?\n' + [Regex]::Escape($profileEnd) + '(?:\r?\n)?\z'
if ($profileExists -and $profileOriginal -and
    (-not [Regex]::IsMatch($profileOriginal, $profilePattern) -or
     [Regex]::Matches($profileOriginal, [Regex]::Escape($profileBegin)).Count -ne 1 -or
     [Regex]::Matches($profileOriginal, [Regex]::Escape($profileEnd)).Count -ne 1)) {
    throw 'An unmanaged rurixforge.config.toml already exists. Both configuration files were preserved.'
}
$profileUpdated = (@(
    $profileBegin
    'model_provider = "rurixforge"'
    ('model = ' + (ConvertTo-TomlString $model))
    'model_reasoning_effort = "medium"'
    'service_tier = "default"'
    $profileEnd
) -join $newline) + $newline

function Write-ClientConfiguration([string]$Path, [string]$Before, [string]$After, [bool]$Existed, [bool]$Bom) {
    if ($After -ceq $Before) {
        Write-Host ('Already configured: ' + $Path)
        return
    }
    $directory = [IO.Path]::GetDirectoryName($Path)
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    $temporary = Join-Path $directory ('config.rurixforge-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    try {
        [IO.File]::WriteAllText($temporary, $After, [Text.UTF8Encoding]::new($Bom))
        if ($Existed) {
            if ([IO.File]::ReadAllText($Path) -cne $Before) {
                throw 'Codex configuration changed concurrently. No configuration was replaced; rerun this script.'
            }
            $backup = $Path + '.before-rurixforge-' + (Get-Date -Format 'yyyyMMddTHHmmssfff') + '-' + [Guid]::NewGuid().ToString('N') + '.bak'
            [IO.File]::Replace($temporary, $Path, $backup)
            Write-Host ('Previous configuration saved to: ' + $backup)
        } else {
            [IO.File]::Move($temporary, $Path)
        }
    } finally {
        if ([IO.File]::Exists($temporary)) { Remove-Item -LiteralPath $temporary }
    }
}
Write-ClientConfiguration $configPath $original $updated $exists $hasBom
Write-ClientConfiguration $profilePath $profileOriginal $profileUpdated $profileExists $false
Write-Host ('Configured Codex profile rurixforge in: ' + $profilePath)
Write-Host 'Use: codex --profile rurixforge (load the user environment in existing terminals first).'
