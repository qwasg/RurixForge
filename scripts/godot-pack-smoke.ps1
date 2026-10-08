#requires -Version 5.1
# Standalone Godot package smoke. Requires built forge-agentd/godot-host and a generated runtime.
# Example: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\godot-pack-smoke.ps1
param(
    [string]$RuntimeDir = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\godot-runtime'),
    [string]$AgentdExe = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\debug\forge-agentd.exe'),
    [ValidateSet('forward_plus','mobile','gl_compatibility')][string]$Method = 'forward_plus',
    [ValidateSet('d3d12','vulkan','opengl3')][string]$Driver = 'd3d12',
    [string]$EvidenceDir = (Join-Path (Split-Path -Parent $PSScriptRoot) ('evidence\godot-backend\pack-smoke-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))),
    [int]$TimeoutSec = 90
)
$ErrorActionPreference = 'Stop'
$stamp = [Guid]::NewGuid().ToString('N')
$scratch = Join-Path ([IO.Path]::GetTempPath()) "forge-godot-pack-smoke-$stamp"
$project = Join-Path $scratch 'source-project'
$pack = Join-Path $scratch 'pack-output'
$relocated = Join-Path $scratch 'relocated-portable'
$data = Join-Path $scratch 'agentd-data'
$genData = Join-Path $scratch 'generation-data'
$agentd = $null; $runner = $null; $hostPid = $null
$stdoutFile = $null; $stderrFile = $null; $stdoutTask = $null; $stderrTask = $null
$smokePassed = $false
$oldAddr = $env:FORGE_AGENTD_ADDR; $oldData = $env:FORGE_AGENTD_DATA_DIR
$oldGenData = $env:FORGE_GEN_DATA_DIR; $oldRuntime = $env:FORGE_GODOT_RUNTIME_DIR
function Read-Exact([IO.Stream]$Stream, [byte[]]$Buffer) {
    $offset = 0
    while ($offset -lt $Buffer.Length) { $count = $Stream.Read($Buffer, $offset, $Buffer.Length - $offset); if ($count -le 0) { throw 'RPC stream closed before a complete frame arrived' }; $offset += $count }
}
function Invoke-Rpc([IO.Stream]$Stream, [string]$Method, $Params) {
    $body = [Text.Encoding]::UTF8.GetBytes((@{jsonrpc='2.0';id=1;method=$Method;params=$Params} | ConvertTo-Json -Depth 12 -Compress))
    $header = [BitConverter]::GetBytes([uint32]$body.Length); $Stream.Write($header,0,4); $Stream.Write($body,0,$body.Length); $Stream.Flush()
    $responseHeader = New-Object byte[] 4; Read-Exact $Stream $responseHeader
    $responseBody = New-Object byte[] ([BitConverter]::ToUInt32($responseHeader,0)); Read-Exact $Stream $responseBody
    $response = [Text.Encoding]::UTF8.GetString($responseBody) | ConvertFrom-Json
    if ($response.error) { throw "$Method failed: $($response.error.message)" }; return $response.result
}
function Stop-Tree([int]$ProcessId) {
    if ($ProcessId -gt 0 -and (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) {
        $null = Start-Process -FilePath 'taskkill.exe' -ArgumentList @('/PID',"$ProcessId",'/T','/F') -Wait -WindowStyle Hidden -PassThru
    }
}
function Get-FreePort {
    $listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback,0); $listener.Start()
    $port = $listener.LocalEndpoint.Port; $listener.Stop(); return $port
}
function Write-FixturePng([string]$Path) {
    # Use the platform PNG encoder so the fixture is fully decodable, not only header-valid.
    Add-Type -AssemblyName System.Drawing
    $bitmap = New-Object System.Drawing.Bitmap 16, 8
    try {
        for ($y = 0; $y -lt 8; $y++) {
            for ($x = 0; $x -lt 16; $x++) {
                if ($x -lt 8) { $color = [System.Drawing.Color]::FromArgb(255, 220, 40, 40) }
                else { $color = [System.Drawing.Color]::FromArgb(255, 40, 200, 80) }
                $bitmap.SetPixel($x, $y, $color)
            }
        }
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $bitmap.Dispose() }
}
function Assert-Color([byte[]]$Pixels, [int]$Width, [int]$X, [int]$Y, [int[]]$Expected, [string]$Label) {
    $index = (($Y * $Width) + $X) * 4
    if ($index -lt 0 -or $index + 3 -ge $Pixels.Length) { throw "$Label sample is outside the returned frame" }
    for ($channel = 0; $channel -lt 3; $channel++) {
        if ([Math]::Abs([int]$Pixels[$index + $channel] - $Expected[$channel]) -gt 3) {
            throw "$Label color mismatch: actual=[$($Pixels[$index]),$($Pixels[$index + 1]),$($Pixels[$index + 2])] expected=[$($Expected -join ',')]"
        }
    }
}
try {
    if (-not (Test-Path -LiteralPath $AgentdExe -PathType Leaf)) { throw "Missing forge-agentd executable: $AgentdExe (build with cargo build -p forge-agentd)" }
    if (-not (Test-Path -LiteralPath (Join-Path $RuntimeDir 'runtime-manifest.json') -PathType Leaf)) { throw "Missing Godot runtime: $RuntimeDir (run scripts\godot-runtime.ps1 -Build)" }
    New-Item -ItemType Directory -Force $project, (Join-Path $project 'Content\Scenes'), (Join-Path $project 'Content\Textures'), (Join-Path $project 'Content\Sprites'), (Join-Path $project 'Content\Models'), $data, $genData, $EvidenceDir | Out-Null
    $EvidenceDir = (Resolve-Path -LiteralPath $EvidenceDir).Path
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $forge = @'
[project]
name = "portable-godot-smoke"
mode = "3d"

[dirs]
content = "Content"
scripts = "Content/Scripts"

[render]
backend = "godot"
method = "forward_plus"
driver = "d3d12"
'@
    $forge = $forge.Replace('method = "forward_plus"', ('method = "' + $Method + '"')).Replace('driver = "d3d12"', ('driver = "' + $Driver + '"'))
    [IO.File]::WriteAllText((Join-Path $project 'forge.toml'),$forge,$utf8)

    # The scene deliberately references three real asset kinds: a PNG-backed sprite,
    # an RX model bundle, and their GUID metadata. The model is placed outside the
    # camera so it validates loading/packaging without changing the color probes.
    $textureGuid = '11111111-1111-4111-8111-111111111111'
    $spriteGuid = '22222222-2222-4222-8222-222222222222'
    $modelGuid = '33333333-3333-4333-8333-333333333333'
    $texturePath = Join-Path $project 'Content\Textures\pack-atlas.png'
    Write-FixturePng $texturePath
    [IO.File]::WriteAllText((Join-Path $project 'Content\Textures\pack-atlas.png.meta'),("guid: $textureGuid`ntype: texture`nimporter: texture`n"),$utf8)
    $sprite = (@{version=1;texture=$textureGuid;pivot=@(0.5,0.5);frames=@{full=@{bbox=@(0,0,16,8)}}} | ConvertTo-Json -Depth 10 -Compress)
    [IO.File]::WriteAllText((Join-Path $project 'Content\Sprites\pack-sprite.rxsprite'),$sprite,$utf8)
    [IO.File]::WriteAllText((Join-Path $project 'Content\Sprites\pack-sprite.rxsprite.meta'),("guid: $spriteGuid`ntype: sprite`nimporter: sprite`n"),$utf8)
    $model = @"
{"version":1,"guid":"$modelGuid","revision":2,"name":"pack-model","sourceId":"pack-smoke","sourceHash":"pack-smoke-v1","kind":"prop","roots":[0],"primitives":[{"id":"q","positions":[[-0.5,-0.5,0.0],[0.5,-0.5,0.0],[0.5,0.5,0.0],[-0.5,0.5,0.0]],"normals":[[0.0,0.0,1.0],[0.0,0.0,1.0],[0.0,0.0,1.0],[0.0,0.0,1.0]],"tangents":[[1.0,0.0,0.0,1.0],[1.0,0.0,0.0,1.0],[1.0,0.0,0.0,1.0],[1.0,0.0,0.0,1.0]],"uv0":[[0.0,1.0],[1.0,1.0],[1.0,0.0],[0.0,0.0]],"indices":[0,1,2,0,2,3],"joints":[],"weights":[],"material":0}],"nodes":[{"id":"q","name":"q","children":[],"primitives":[0],"translation":[0.0,0.0,0.0],"rotation":[0.0,0.0,0.0,1.0],"scale":[1.0,1.0,1.0],"matrix":null,"skin":null,"collision":false}],"materials":[{"guid":"44444444-4444-4444-8444-444444444444","name":"white","baseColor":[1.0,1.0,1.0,1.0],"metallic":0.0,"roughness":1.0,"emissive":[0.0,0.0,0.0],"baseColorTexture":null,"normalTexture":null,"metallicRoughnessTexture":null,"occlusionTexture":null,"emissiveTexture":null,"normalScale":1.0,"occlusionStrength":1.0,"doubleSided":false,"alphaMode":"OPAQUE","alphaCutoff":0.5,"unlit":true}],"textures":[],"skins":[],"animations":[],"idleClip":"","walkClip":""}
"@
    [IO.File]::WriteAllText((Join-Path $project 'Content\Models\pack-model.rxmodel'),$model.Trim(),$utf8)
    [IO.File]::WriteAllText((Join-Path $project 'Content\Models\pack-model.rxmodel.meta'),("guid: $modelGuid`ntype: model`nimporter: model`n"),$utf8)
    $history = Join-Path $project ('.forge\cache\models\' + $modelGuid)
    New-Item -ItemType Directory -Force $history | Out-Null
    [IO.File]::WriteAllText((Join-Path $history '1.rxmodel'),$model.Replace('"revision":2','"revision":1').Replace('"baseColor":[1.0,1.0,1.0,1.0]','"baseColor":[0.04,0.08,0.8,1.0]').Trim(),$utf8)
    $scene = @"
{"name":"portable-smoke","next_id":3,"entities":[{"id":1,"name":"textured-sprite","transform":{"translation":[0,0,0],"rotation":[0,0,0,1],"scale":[1,1,1]},"components":[{"type":"Sprite","enabled":true,"props":{"sprite":"$spriteGuid","pixelsPerUnit":8.0,"chromaKey":"none","blendMode":"opaque","tint":[1,1,1,1]}}]},{"id":2,"name":"portable-model","transform":{"translation":[20,0,0],"rotation":[0,0,0,1],"scale":[1,1,1]},"components":[{"type":"ModelRenderer","enabled":true,"props":{"model":"$modelGuid","revision":1}}]}]}
"@
    [IO.File]::WriteAllText((Join-Path $project 'Content\Scenes\smoke.rxscene'),$scene.Trim(),$utf8)
    [IO.File]::WriteAllText((Join-Path $project 'Content\Scenes\smoke.rxscene.meta'),"guid: 55555555-5555-4555-8555-555555555555`ntype: scene`nimporter: scene`n",$utf8)
    $agentdPort = Get-FreePort
    $env:FORGE_AGENTD_ADDR = "127.0.0.1:$agentdPort"; $env:FORGE_AGENTD_DATA_DIR = $data
    $env:FORGE_GEN_DATA_DIR = $genData; $env:FORGE_GODOT_RUNTIME_DIR = $RuntimeDir
    $agentd = Start-Process -FilePath $AgentdExe -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $EvidenceDir 'agentd.stdout.log') -RedirectStandardError (Join-Path $EvidenceDir 'agentd.stderr.log')
    $healthUrl = "http://127.0.0.1:$agentdPort/health"; $ready = $false
    for ($i=0; $i -lt 60; $i++) { try { if ((Invoke-RestMethod -Uri $healthUrl -TimeoutSec 2).status -eq 'ok') { $ready=$true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ready) { throw "forge-agentd failed to become ready at $healthUrl" }
    $packRequest = @{sceneRef=(Join-Path $project 'Content\Scenes\smoke.rxscene');outDir=$pack;port=(Get-FreePort)}
    $report = Invoke-RestMethod -Uri "http://127.0.0.1:$agentdPort/api/forge/project/pack" -Method Post -ContentType 'application/json' -Body ($packRequest | ConvertTo-Json -Compress) -TimeoutSec 120
    if ($report.backend -ne 'godot') { throw "Expected Godot pack, got $($report.backend)" }
    foreach ($required in @('forge.toml','Content/Scenes/smoke.rxscene','Content/Scenes/smoke.rxscene.meta','Content/Sprites/pack-sprite.rxsprite','Content/Sprites/pack-sprite.rxsprite.meta','Content/Textures/pack-atlas.png','Content/Textures/pack-atlas.png.meta','Content/Models/pack-model.rxmodel','Content/Models/pack-model.rxmodel.meta',('.forge/cache/models/' + $modelGuid + '/1.rxmodel'),'runtime/forge-godot_console.exe','runtime/bin/godot_host.dll','runtime/runtime-manifest.json','pack-run.ps1')) {
        if (-not (Test-Path -LiteralPath (Join-Path $pack $required) -PathType Leaf)) { throw "Pack is incomplete: missing $required" }
    }
    $expectedClosure = @('Content/Scenes/smoke.rxscene','Content/Scenes/smoke.rxscene.meta','Content/Sprites/pack-sprite.rxsprite','Content/Sprites/pack-sprite.rxsprite.meta','Content/Textures/pack-atlas.png','Content/Textures/pack-atlas.png.meta','Content/Models/pack-model.rxmodel','Content/Models/pack-model.rxmodel.meta')
    foreach ($required in $expectedClosure) {
        if (-not (@($report.closure) -contains $required)) { throw "Pack closure omitted referenced asset: $required" }
    }
    $packedScene = Get-Content -LiteralPath (Join-Path $pack 'Content\Scenes\smoke.rxscene') -Raw | ConvertFrom-Json
    if (@($packedScene.entities).Count -ne 2) { throw 'Packed scene fixture was unexpectedly rewritten or truncated' }
    $launcher = Get-Content -LiteralPath (Join-Path $pack 'pack-run.ps1') -Raw
    foreach ($setting in @('FORGE_PROJECT_ROOT','FORGE_HOST_PORT','FORGE_GAME_SCENE','FORGE_RENDER_BACKEND','FORGE_RENDER_METHOD','FORGE_RENDER_DRIVER','FORGE_GODOT_RUNTIME_DIR')) {
        if ($launcher.IndexOf($setting, [System.StringComparison]::Ordinal) -lt 0) { throw "Portable launcher does not set $setting explicitly" }
    }
    Copy-Item -LiteralPath $pack -Destination $relocated -Recurse -Force
    Remove-Item -LiteralPath $project -Recurse -Force
    $stdoutPath = Join-Path $EvidenceDir 'portable.stdout.log'; $stderrPath = Join-Path $EvidenceDir 'portable.stderr.log'
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $start.Arguments = '-NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $relocated 'pack-run.ps1') + '"'
    $start.UseShellExecute=$false; $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true; $start.CreateNoWindow=$true
    $start.WorkingDirectory = $data # unrelated to both checkout and portable package
    foreach ($key in @($start.EnvironmentVariables.Keys)) {
        if ([string]$key -like 'FORGE_*') { $start.EnvironmentVariables.Remove([string]$key) }
    }
    $runner = New-Object Diagnostics.Process; $runner.StartInfo=$start; $runner.Start() | Out-Null
    $stdoutFile=[IO.File]::Create($stdoutPath); $stderrFile=[IO.File]::Create($stderrPath)
    $stdoutTask=$runner.StandardOutput.BaseStream.CopyToAsync($stdoutFile); $stderrTask=$runner.StandardError.BaseStream.CopyToAsync($stderrFile)
    $tcp=$null; $deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSec)
    while ([DateTime]::UtcNow -lt $deadline) {
        $runner.Refresh(); if ($runner.HasExited) { throw "Portable launcher exited ($($runner.ExitCode)): $(if(Test-Path $stderrPath){Get-Content $stderrPath -Raw})" }
        try { $tcp=New-Object Net.Sockets.TcpClient; $tcp.Connect('127.0.0.1',[int]$packRequest.port); break }
        catch { if($null -ne $tcp){$tcp.Close();$tcp=$null}; Start-Sleep -Milliseconds 300 }
    }
    if ($null -eq $tcp) { throw "Portable Godot package did not listen within $TimeoutSec seconds" }
    $stream=$tcp.GetStream(); $stream.ReadTimeout=30000; $stream.WriteTimeout=30000
    try {
        $ping=Invoke-Rpc $stream 'host.ping' @{}; if($ping.pong -ne $true){throw 'host.ping did not return pong'}; $hostPid=[int]$ping.pid
        $backend=Invoke-Rpc $stream 'render.backendInfo' @{}; if($backend.renderBackend -ne 'godot'){throw "Wrong render backend: $($backend | ConvertTo-Json -Compress)"}
        if($backend.method -ne $Method -or $backend.driver -ne $Driver){throw "Wrong runtime configuration: $($backend | ConvertTo-Json -Compress)"}
        Invoke-Rpc $stream 'viewport.setCamera' @{target=@(0,0,0);yaw=0;pitch=0;dist=10;ortho=$true;orthoSize=1.5} | Out-Null
        $summary=Invoke-Rpc $stream 'scene.summary' @{}; if([int]$summary.entityCount -ne 2){throw "Pack scene failed to load both sprite and model: $($summary | ConvertTo-Json -Compress)"}
        $frame=Invoke-Rpc $stream 'viewport.frame' @{width=64;height=48}
        if([int]$frame.width -ne 64 -or [int]$frame.height -ne 48 -or [string]::IsNullOrEmpty($frame.pixelsB64)){throw "Portable runtime failed to return a frame: $($frame | ConvertTo-Json -Compress)"}
        $pixels=[Convert]::FromBase64String($frame.pixelsB64)
        if($pixels.Length -ne 64*48*4){throw 'Unexpected RGBA frame length'}
        if([int]$frame.draws -ne 2){throw "Portable scene must submit both sprite and model draws: $($frame.draws)"}
        # With the explicit camera and 16x8 atlas at 8 pixels/unit, the sprite spans
        # x=16..48 and its left/right halves must retain their source colors.
        Assert-Color $pixels 64 24 24 @(220,40,40) 'sprite left region'
        Assert-Color $pixels 64 40 24 @(40,200,80) 'sprite right region'
        $redPixels=0; $greenPixels=0
        for($i=0;$i -lt $pixels.Length;$i+=4){
            if($pixels[$i] -gt 160 -and $pixels[$i+1] -lt 90 -and $pixels[$i+2] -lt 90){$redPixels++}
            if($pixels[$i] -lt 90 -and $pixels[$i+1] -gt 140 -and $pixels[$i+2] -lt 130){$greenPixels++}
        }
        if($redPixels -lt 40 -or $greenPixels -lt 40){throw "Texture spatial coverage is missing: redPixels=$redPixels greenPixels=$greenPixels draws=$($frame.draws)"}
        # Inspect revision 1 with the camera; standalone game mode correctly
        # forbids transform edits. Current revision 2 is white, historical 1 blue.
        Invoke-Rpc $stream 'viewport.setCamera' @{target=@(20,0,0);yaw=0;pitch=0;dist=10;ortho=$true;orthoSize=1.5} | Out-Null
        $modelFrame=Invoke-Rpc $stream 'viewport.frame' @{width=64;height=48}
        $modelPixels=[Convert]::FromBase64String($modelFrame.pixelsB64)
        $center=(24*64+32)*4
        $modelColor=@([int]$modelPixels[$center],[int]$modelPixels[$center+1],[int]$modelPixels[$center+2])
        if($modelColor[2] -lt 120 -or $modelColor[2] -lt $modelColor[0]+40 -or $modelColor[2] -lt $modelColor[1]+30){throw "Historical blue model failed to render: $($modelColor -join ',')"}
        Invoke-Rpc $stream 'viewport.setCamera' @{target=@(40,0,0);yaw=0;pitch=0;dist=10;ortho=$true;orthoSize=1.5} | Out-Null
        $emptyFrame=Invoke-Rpc $stream 'viewport.frame' @{width=64;height=48}
        $emptyPixels=[Convert]::FromBase64String($emptyFrame.pixelsB64)
        Assert-Color $emptyPixels 64 32 24 @([int]$emptyPixels[0],[int]$emptyPixels[1],[int]$emptyPixels[2]) 'camera outside model restores background'
        $backend=Invoke-Rpc $stream 'render.backendInfo' @{}
        [IO.File]::WriteAllText((Join-Path $EvidenceDir 'report.json'),(@{passed=$true;backend=$backend;scene=$summary;redPixels=$redPixels;greenPixels=$greenPixels;modelRevision=1;modelColor=$modelColor;draws=$frame.draws;sourceRemoved=(-not(Test-Path $project));unrelatedCwd=$data;forgeOverridesCleared=$true;packageReport=$report} | ConvertTo-Json -Depth 30),$utf8)
    } finally { if($null -ne $stream){$stream.Dispose()}; $tcp.Close() }
    $smokePassed=$true; Write-Output 'PASS: relocated Godot package started with cleared overrides, loaded its scene, and returned nonblank pixels.'; Write-Output "Evidence: $EvidenceDir"; exit 0
} catch {
    Write-Output "FAIL: $_"
    Write-Output "Evidence retained for diagnosis: $EvidenceDir"
    exit 1
} finally {
    if($hostPid){Stop-Tree $hostPid}
    if($null -ne $runner){try{if(-not $runner.HasExited){Stop-Tree $runner.Id}}catch{};try{$runner.Dispose()}catch{}}
    if($null -ne $agentd){try{if(-not $agentd.HasExited){Stop-Tree $agentd.Id}}catch{}}
    if($null -ne $stdoutFile){try{$stdoutFile.Dispose()}catch{}};if($null -ne $stderrFile){try{$stderrFile.Dispose()}catch{}}
    if($null -ne $stdoutTask){try{$stdoutTask.Wait(2000)|Out-Null}catch{}};if($null -ne $stderrTask){try{$stderrTask.Wait(2000)|Out-Null}catch{}}
    $env:FORGE_AGENTD_ADDR=$oldAddr;$env:FORGE_AGENTD_DATA_DIR=$oldData;$env:FORGE_GEN_DATA_DIR=$oldGenData;$env:FORGE_GODOT_RUNTIME_DIR=$oldRuntime
    if($smokePassed -and (Test-Path -LiteralPath $scratch)){Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue}
}
