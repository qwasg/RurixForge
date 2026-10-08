# rurix frame-hash baseline (Stage 2 step 1; design: docs/godot-backend/02-render-seam-design.md section 9.3)
# Compatible with Windows PowerShell 5.1 (no pwsh on this host): large JSON goes through JavaScriptSerializer.
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\rurix-frame-baseline.ps1                 # write baseline
#   powershell ... -File scripts\rurix-frame-baseline.ps1 -Compare -Label "pass2"                       # re-run and compare
# Exit code: 0 = written / all hashes match, 1 = mismatch or unstable frame, 2 = setup error,
#            3 = the host rendered on a different GPU than the baseline (environment, not code).
# Device pin: rurix takes Vulkan physical device 0 (rurix-rt pick_physical_device), and that order can
# change between runs on a hybrid-GPU laptop. -Compare therefore pins the child process to the
# baseline's GPU through VK_DRIVER_FILES (ICD manifest looked up by DriverDesc in the display-adapter
# class key); -VkDriverFiles overrides the lookup, -NoDevicePin disables it.
param(
    [string]$Exe = '',
    [string]$Out = '',
    [switch]$Compare,
    [string]$Label = '',
    [string]$VkDriverFiles = '',
    [switch]$NoDevicePin,
    [string[]]$Projects = @('projects\demo', 'projects\pvz'),
    [string]$SceneFilter = '*'
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $Exe) { $Exe = Join-Path $repo 'target\debug\engine-host.exe' }
if (-not $Out) { $Out = Join-Path $repo 'evidence\godot-backend\rurix-frame-baseline.json' }
if (-not (Test-Path -LiteralPath $Exe)) { Write-Error "engine-host not found: $Exe"; exit 2 }

Add-Type -AssemblyName System.Web.Extensions
$json = New-Object System.Web.Script.Serialization.JavaScriptSerializer
$json.MaxJsonLength = [int]::MaxValue
$json.RecursionLimit = 256
$sha = [System.Security.Cryptography.SHA256]::Create()
# JavaScriptSerializer cannot walk PowerShell-wrapped objects; it is used only to PARSE large responses.
function To-Json($o) { ConvertTo-Json -InputObject $o -Compress -Depth 10 }

# DCH display drivers register their Vulkan ICD manifest next to DriverDesc under the adapter class key.
function Resolve-VkIcd([string]$device) {
    $cls = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}'
    foreach ($k in (Get-ChildItem $cls -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -match '^\d{4}$' })) {
        $p = Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue
        if ($p -and $p.DriverDesc -eq $device) {
            foreach ($f in @($p.VulkanDriverName)) { if ($f -and (Test-Path -LiteralPath $f)) { return $f } }
        }
    }
    return ''
}
$expectDevice = ''
if ($Compare -and (Test-Path -LiteralPath $Out)) {
    $expectDevice = [string]$json.DeserializeObject([IO.File]::ReadAllText($Out))['entries'][0]['deviceName']
    if (-not $VkDriverFiles -and $expectDevice -and -not $NoDevicePin) {
        $VkDriverFiles = Resolve-VkIcd $expectDevice
        if (-not $VkDriverFiles) { Write-Error "no Vulkan ICD registered for baseline device '$expectDevice' (pass -VkDriverFiles or -NoDevicePin)"; exit 2 }
    }
    Write-Host "baseline device: $expectDevice; VK_DRIVER_FILES: $(if ($VkDriverFiles) { $VkDriverFiles } else { '(not pinned)' })"
}
$Sizes = @(@(960, 540), @(1280, 720), @(320, 180))
# EditorCamera::default (crates/engine-host/src/viewport.rs:207-218); reset before every load so
# "asLoaded" never depends on the previous scene.
$DefaultCam = @{ target = @(0.0, 0.5, 0.0); yaw = 35.0; pitch = 28.0; dist = 9.0; fovY = 50.0; ortho = $false; orthoSize = 5.0 }
$Fixed3d = @{ target = @(0.0, 0.0, 0.0); yaw = 35.0; pitch = -25.0; dist = 12.0; fovY = 60.0; ortho = $false }
$Fixed2d = @{ target = @(0.0, 0.0, 0.0); yaw = 0.0; pitch = 0.0; dist = 10.0; ortho = $true; orthoSize = 5.0 }

function Get-Hex([byte[]]$b) { ([BitConverter]::ToString($sha.ComputeHash($b))).Replace('-', '') }

function Read-Exact($s, [byte[]]$b) {
    $o = 0
    while ($o -lt $b.Length) {
        $k = $s.Read($b, $o, $b.Length - $o)
        if ($k -le 0) { throw 'EOF from engine-host' }
        $o += $k
    }
}

# JSON-RPC over TCP: 4-byte little-endian length + UTF-8 JSON (crates/engine-host/src/frame.rs:1-29).
$script:rpcId = 0
function Invoke-Rpc($s, [string]$method, $params) {
    $script:rpcId++
    $req = @{ jsonrpc = '2.0'; id = $script:rpcId; method = $method; params = $params }
    $body = [Text.Encoding]::UTF8.GetBytes((To-Json $req))
    $s.Write([BitConverter]::GetBytes([uint32]$body.Length), 0, 4)
    $s.Write($body, 0, $body.Length)
    $h = New-Object byte[] 4
    Read-Exact $s $h
    $buf = New-Object byte[] ([BitConverter]::ToUInt32($h, 0))
    Read-Exact $s $buf
    $r = $json.DeserializeObject([Text.Encoding]::UTF8.GetString($buf))
    if ($r.ContainsKey('error') -and $null -ne $r['error']) { throw "${method}: $($r['error']['message'])" }
    return $r['result']
}

function Start-Host([string]$root) {
    $psi = New-Object Diagnostics.ProcessStartInfo($Exe, '--port 0')
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.WorkingDirectory = $repo
    $psi.EnvironmentVariables['FORGE_PROJECT_ROOT'] = $root
    if ($psi.EnvironmentVariables.ContainsKey('FORGE_GPU_PARTICLES')) { $psi.EnvironmentVariables.Remove('FORGE_GPU_PARTICLES') }
    if ($psi.EnvironmentVariables.ContainsKey('FORGE_HOST_PORT')) { $psi.EnvironmentVariables.Remove('FORGE_HOST_PORT') }
    if ($VkDriverFiles) { $psi.EnvironmentVariables['VK_DRIVER_FILES'] = $VkDriverFiles }
    $p = [Diagnostics.Process]::Start($psi)
    $p.BeginErrorReadLine()   # drain stderr (no handler: discarded) so the host never blocks on a full pipe
    $deadline = (Get-Date).AddSeconds(90)
    while ((Get-Date) -lt $deadline) {
        $t = $p.StandardOutput.ReadLineAsync()
        if (-not $t.Wait(30000)) { break }
        $line = $t.Result
        if ($null -eq $line) { break }
        # Scan every line for the prefix (same rule as the MCP supervisor; see 02 section 6.1).
        if ($line.StartsWith('FORGE_HOST_LISTENING port=')) { return @{ proc = $p; port = [int]$line.Substring(26) } }
    }
    try { $p.Kill() } catch {}
    throw "engine-host did not become ready for $root"
}


# viewport.frame with a bounded retry: a size-only rebuild inside the 1500 ms debounce window makes
# engine-host return a readback-length error once (viewport.rs:1884-1903, :2113-2115). Retry after the
# window; anything that still fails after 3 tries is a real error and is rethrown.
function Get-Frame($s, $q) {
    for ($i = 0; ; $i++) {
        try {
            $f = Invoke-Rpc $s 'viewport.frame' $q
            $f['_retries'] = $i
            return $f
        } catch {
            if ($i -ge 3) { throw }
            Start-Sleep -Milliseconds 1700
        }
    }
}

function Get-BaselineScenes([string]$root) {
    # entry-scene from forge.toml (default Content/Scenes/Main.rxscene), then every Content/**/*.rxscene.
    $entry = 'Content/Scenes/Main.rxscene'
    $ft = Join-Path $root 'forge.toml'
    if (Test-Path -LiteralPath $ft) {
        foreach ($l in (Get-Content -LiteralPath $ft -Encoding UTF8)) {
            if ($l -match '^\s*entry-scene\s*=\s*"([^"]+)"') { $entry = $Matches[1]; break }
        }
    }
    $list = New-Object System.Collections.Generic.List[string]
    $list.Add(($entry -replace '\\', '/'))
    $content = Join-Path $root 'Content'
    if (Test-Path -LiteralPath $content) {
        Get-ChildItem -LiteralPath $content -Recurse -File -Filter '*.rxscene' | Sort-Object FullName | ForEach-Object {
            $rel = $_.FullName.Substring($root.Length + 1) -replace '\\', '/'
            if (-not $list.Contains($rel)) { $list.Add($rel) }
        }
    }
    return @($list | Where-Object { $_ -like $SceneFilter })
}

function Measure-Project([string]$proj) {
    $root = Join-Path $repo $proj
    $scenes = Get-BaselineScenes $root
    $h = Start-Host $root
    $rows = New-Object System.Collections.Generic.List[object]
    try {
        $client = New-Object Net.Sockets.TcpClient('127.0.0.1', $h.port)
        $client.ReceiveTimeout = 120000
        $s = $client.GetStream()
        # Sizes are the OUTER loop: engine-host defers a size-only session rebuild that comes within
        # 1500 ms of the previous rebuild, and then fails the readback length check
        # (crates/engine-host/src/viewport.rs:1884-1903 and :2113-2115). Scene switches rebuild for
        # "sig" and are not deferred, so only the size switch needs a settle delay.
        foreach ($wh in $Sizes) {
            Start-Sleep -Milliseconds 1700
            foreach ($scene in $scenes) {
                $null = Invoke-Rpc $s 'viewport.setCamera' $DefaultCam          # scene order must not leak into asLoaded
                $info = Invoke-Rpc $s 'scene.load' @{ path = (Join-Path $root $scene) }
                $loadedCam = Invoke-Rpc $s 'viewport.getCamera' @{}
                foreach ($cam in @('asLoaded', 'fixed')) {
                    if ($cam -eq 'fixed') {
                        $fx = if ($loadedCam['ortho']) { $Fixed2d } else { $Fixed3d }
                        $null = Invoke-Rpc $s 'viewport.setCamera' $fx
                    }
                    $q = @{ width = $wh[0]; height = $wh[1]; format = 'rgba8' }
                    $a = Get-Frame $s $q
                    if ($expectDevice -and $a['deviceName'] -ne $expectDevice) {
                        Write-Host "DEVICE MISMATCH: baseline='$expectDevice' now='$($a['deviceName'])' (environment, not code)"
                        try { $h.proc.Kill() } catch {}
                        exit 3
                    }
                    $b = Get-Frame $s $q
                    $ha = Get-Hex ([Convert]::FromBase64String($a['pixelsB64']))
                    $hb = Get-Hex ([Convert]::FromBase64String($b['pixelsB64']))
                    $rows.Add([ordered]@{
                        key = "$proj|$scene|$cam|$($a['width'])x$($a['height'])"
                        project = $proj; scene = $scene; mode = $info['mode']; camera = $cam
                        width = $a['width']; height = $a['height']; sha256 = $ha; stable = ($ha -eq $hb)
                        retries = ($a['_retries'] + $b['_retries'])
                        draws = $a['draws']; triangles = $a['triangles']; truncated = $a['truncated']
                        meshFallbacks = $a['meshFallbacks']; meshClasses = $a['meshClasses']
                        nonZeroPixels = $a['nonZeroPixels']; deviceName = $a['deviceName']; framePath = $a['framePath']
                    })
                }
            }
            Write-Host ("  {0} {1}x{2}: {3} scenes ok" -f $proj, $wh[0], $wh[1], $scenes.Count)
        }
        $client.Close()
    } finally {
        try { $h.proc.Kill() } catch {}
    }
    return $rows
}


# ---- main ----
$started = Get-Date
$all = New-Object System.Collections.Generic.List[object]
foreach ($proj in $Projects) {
    Write-Host "project $proj"
    foreach ($r in (Measure-Project $proj)) { $all.Add($r) }
}
$rustc = (& rustc --version 2>$null)
$run = [ordered]@{
    label = $(if ($Label) { $Label } elseif ($Compare) { 'compare' } else { 'baseline' })
    createdAt = $started.ToString('o'); seconds = [int]((Get-Date) - $started).TotalSeconds
    exeSha256 = (Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash
    rustcInRepo = $rustc
    gitHead = (git -C $repo rev-parse HEAD); dirtyFiles = @(git -C $repo status --porcelain).Count
    frames = $all.Count; unstable = @($all | Where-Object { -not $_['stable'] }).Count
    deviceName = $(if ($all.Count) { $all[0]['deviceName'] } else { '' }); vkDriverFiles = $VkDriverFiles
}
$unstable = @($all | Where-Object { -not $_['stable'] })

function Save-Doc($doc) {
    New-Item -ItemType Directory -Force (Split-Path -Parent $Out) | Out-Null
    # One entry per line keeps the evidence file diffable.
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add('{')
    $lines.Add('"schema": 1,')
    $lines.Add('"design": "docs/godot-backend/02-render-seam-design.md section 9.3",')
    $lines.Add('"baseline": ' + (To-Json $doc['baseline']) + ',')
    $lines.Add('"runs": [')
    $runs = New-Object System.Collections.Generic.List[object]
    foreach ($x in $doc['runs']) { $runs.Add($x) }        # PS 5.1: @() on a list of ordered dictionaries throws
    for ($i = 0; $i -lt $runs.Count; $i++) { $lines.Add('  ' + (To-Json $runs[$i]) + $(if ($i -lt $runs.Count - 1) { ',' } else { '' })) }
    $lines.Add('],')
    $lines.Add('"entries": [')
    $ents = New-Object System.Collections.Generic.List[object]
    foreach ($x in $doc['entries']) { $ents.Add($x) }
    for ($i = 0; $i -lt $ents.Count; $i++) { $lines.Add('  ' + (To-Json $ents[$i]) + $(if ($i -lt $ents.Count - 1) { ',' } else { '' })) }
    $lines.Add(']')
    $lines.Add('}')
    [IO.File]::WriteAllLines($Out, $lines, (New-Object Text.UTF8Encoding($false)))
}

if (-not $Compare) {
    if ($unstable.Count) {
        Write-Host "UNSTABLE frames: $($unstable.Count) (first: $($unstable[0]['key']))"
        exit 1
    }
    $run['result'] = 'baseline-written'
    Save-Doc @{ baseline = $run; runs = @($run); entries = $all }
    Write-Host "baseline written: $Out ($($all.Count) frames, $($run.seconds) s)"
    exit 0
}

if (-not (Test-Path -LiteralPath $Out)) { Write-Error "no baseline at $Out"; exit 2 }
$old = $json.DeserializeObject([IO.File]::ReadAllText($Out))
$byKey = @{}
foreach ($e in $old['entries']) { $byKey[$e['key']] = $e['sha256'] }
$mismatch = New-Object System.Collections.Generic.List[string]
foreach ($e in $all) {
    if (-not $byKey.ContainsKey($e['key'])) { $mismatch.Add("new:$($e['key'])") }
    elseif ($byKey[$e['key']] -ne $e['sha256']) { $mismatch.Add("diff:$($e['key'])") }
}
if ($old['entries'].Count -ne $all.Count) { $mismatch.Add("count: baseline=$($old['entries'].Count) now=$($all.Count)") }
$run['mismatches'] = @($mismatch | Select-Object -First 50)
$run['mismatchCount'] = $mismatch.Count
$run['result'] = $(if ($mismatch.Count -eq 0 -and $unstable.Count -eq 0) { 'match' } else { 'MISMATCH' })
$newRuns = New-Object System.Collections.Generic.List[object]
foreach ($x in $old['runs']) { $newRuns.Add($x) }
$newRuns.Add($run)
$old['runs'] = $newRuns
Save-Doc $old
Write-Host "compare [$($run.label)]: $($run.result) mismatches=$($mismatch.Count) unstable=$($unstable.Count) frames=$($all.Count)"
if ($run['result'] -eq 'match') { exit 0 } else { exit 1 }
