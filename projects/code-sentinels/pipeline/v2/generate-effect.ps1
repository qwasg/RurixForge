param([Parameter(Mandatory=$true)][ValidateSet('deepseek-tide','gpt-nova','pycharm-matrix')][string]$Effect)
$ErrorActionPreference='Stop'
$project=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$spec=Get-Content -LiteralPath (Join-Path $PSScriptRoot "$Effect.request.json") -Raw|ConvertFrom-Json
$receiptPath=Join-Path $PSScriptRoot "$Effect.video.json"
$attemptPath=Join-Path $PSScriptRoot "$Effect.attempt.json"
$api='http://127.0.0.1:8103/api/forge'
$workspace='ws_1788669812422_35134176'
if(Test-Path -LiteralPath $receiptPath){$receipt=Get-Content -LiteralPath $receiptPath -Raw|ConvertFrom-Json}
else {
  if(Test-Path -LiteralPath $attemptPath){throw 'Existing paid VFX submission recorded; inspect the result before any retry.'}
  if(-not $spec.uploaded.uri){throw 'Explicit private reference upload is required.'}
  $payload=@{workspaceId=$workspace;backend='aliyun-minimax-video';prompt=$spec.prompt;imageDataUrl=$spec.uploaded.uri;aspect='1:1';resolution='768p';durationSec=6}
  @{startedAt=[DateTime]::UtcNow.ToString('o');imageRef=$spec.imageRef;mode='image2video';executor='RurixForge Codex agent'}|ConvertTo-Json|Set-Content -LiteralPath $attemptPath -Encoding utf8
  Write-Output "$Effect real MiniMax-H3 VFX image-to-video submitted; awaiting provider."
  try {
    $raw=Invoke-RestMethod "$api/gen/video" -Method Post -ContentType 'application/json; charset=utf-8' -Body ($payload|ConvertTo-Json -Depth 10 -Compress) -TimeoutSec 1050
    $receipt=@{backendId=$raw.backendId;artifacts=@($raw.artifacts|ForEach-Object {@{fileRef=$_.fileRef;ext=$_.ext;mime=$_.mime;meta=$_.meta}})}
    $receipt|ConvertTo-Json -Depth 15|Set-Content -LiteralPath $receiptPath -Encoding utf8
  }catch {
    @{error=$_.Exception.Message;detail=$_.ErrorDetails.Message}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $PSScriptRoot "$Effect.failure.json") -Encoding utf8
    throw
  }
}
$video=$receipt.artifacts[0]
if($video.meta.mode -ne 'image2video'){throw 'Expected real first-frame image-to-video provider receipt.'}
$videoPath=Join-Path $project $video.fileRef
if(-not(Test-Path -LiteralPath $videoPath)){throw 'Generated provider video missing.'}
$permanent=Join-Path $project 'SourceMedia/effects'
New-Item -ItemType Directory -Force $permanent|Out-Null
Copy-Item -LiteralPath $videoPath -Destination (Join-Path $permanent "$Effect.mp4") -Force
Write-Output "$Effect video saved permanently; decoding 72 real frames at 12fps with black-background energy alpha."
$frameBody=@{workspaceId=$workspace;videoFileRef=$video.fileRef;fps=12;maxFrames=72;chromaKey='black';crop='none';padding=2}|ConvertTo-Json -Compress
$frames=Invoke-RestMethod "$api/gen/video/frames" -Method Post -ContentType 'application/json; charset=utf-8' -Body $frameBody -TimeoutSec 240
if($frames.frameCount -ne 72){throw 'Expected 72 actual source frames to cover the full six-second effect.'}
$clean=@{atlas=@{fileRef=$frames.atlas.fileRef;mime=$frames.atlas.mime;width=$frames.atlas.width;height=$frames.atlas.height};boxes=$frames.boxes;fps=$frames.fps;frameCount=$frames.frameCount}
$clean|ConvertTo-Json -Depth 15|Set-Content -LiteralPath (Join-Path $PSScriptRoot "$Effect.frames.json") -Encoding utf8
@{effect=$Effect;taskId=$video.meta.taskId;frames=$frames.frameCount;sourceFps=$frames.fps;video=(Join-Path $permanent "$Effect.mp4");rawAlphaAtlas=$frames.atlas.fileRef}|ConvertTo-Json -Compress
