param([Parameter(Mandatory=$true)][ValidateSet('deepseek','gpt','gpt-v2')][string]$Character)
$ErrorActionPreference='Stop'
$project=Split-Path -Parent $PSScriptRoot
$spec=Get-Content -LiteralPath (Join-Path $PSScriptRoot "$Character.request.json") -Raw | ConvertFrom-Json
$receiptPath=Join-Path $PSScriptRoot "$Character.video.json"
$attemptPath=Join-Path $PSScriptRoot "$Character.attempt.json"
$api='http://127.0.0.1:8103/api/forge'
$workspace='ws_1788669812422_35134176'
if(Test-Path -LiteralPath $receiptPath) {
  $receipt=Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
} else {
  if(Test-Path -LiteralPath $attemptPath) { throw 'Existing paid submission recorded: inspect attempt/result, never submit a duplicate.' }
  if(-not $spec.uploaded.uri) { throw 'Private first-frame upload is required.' }
  $payload=@{workspaceId=$workspace;backend='aliyun-minimax-video';prompt=$spec.prompt;imageDataUrl=$spec.uploaded.uri;aspect='1:1';resolution='768p';durationSec=6}
  @{startedAt=[DateTime]::UtcNow.ToString('o');imageRef=$spec.imageRef;mode='image2video';executor='RurixForge Codex agent'} | ConvertTo-Json | Set-Content -LiteralPath $attemptPath -Encoding utf8
  Write-Output "$Character real image-to-video submitted via RurixForge; awaiting provider."
  try {
    $raw=Invoke-RestMethod "$api/gen/video" -Method Post -ContentType 'application/json; charset=utf-8' -Body ($payload|ConvertTo-Json -Depth 10 -Compress) -TimeoutSec 1050
    $receipt=@{backendId=$raw.backendId;artifacts=@($raw.artifacts | ForEach-Object {@{fileRef=$_.fileRef;ext=$_.ext;mime=$_.mime;meta=$_.meta}})}
    $receipt | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath $receiptPath -Encoding utf8
  } catch {
    @{error=$_.Exception.Message;detail=$_.ErrorDetails.Message} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $PSScriptRoot "$Character.failure.json") -Encoding utf8
    throw
  }
}
$video=$receipt.artifacts[0]
if($video.meta.mode -ne 'image2video') { throw 'Provider receipt is not image-to-video.' }
$videoPath=Join-Path $project $video.fileRef
if(-not(Test-Path -LiteralPath $videoPath)) { throw 'Provider video is missing from selected project.' }
Write-Output "$Character video ready; extracting 32 actual video frames with ffmpeg."
$frameBody=@{workspaceId=$workspace;videoFileRef=$video.fileRef;fps=8;maxFrames=32;chromaKey='magenta';crop='union';padding=2}|ConvertTo-Json -Compress
$frames=Invoke-RestMethod "$api/gen/video/frames" -Method Post -ContentType 'application/json; charset=utf-8' -Body $frameBody -TimeoutSec 180
if($frames.frameCount -ne 32) { throw 'Actual frame count does not meet requirement.' }
$clean=@{atlas=@{fileRef=$frames.atlas.fileRef;mime=$frames.atlas.mime;width=$frames.atlas.width;height=$frames.atlas.height};boxes=$frames.boxes;fps=$frames.fps;frameCount=$frames.frameCount}
$clean | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath (Join-Path $PSScriptRoot "$Character.frames.json") -Encoding utf8
$out=Join-Path $project 'public/assets/characters'
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item -LiteralPath (Join-Path $project $frames.atlas.fileRef) -Destination (Join-Path $out "$Character.png") -Force
$metadata=@{id=$Character;image="$Character.png";width=$frames.atlas.width;height=$frames.atlas.height;fps=$frames.fps;frameCount=$frames.frameCount;boxes=$frames.boxes;frames=$frames.boxes;pivot=@(0.5,1.0);crop='union';chromaKey='magenta';provenance=@{method='image-to-video-extracted-frames';backend=$receipt.backendId;taskId=$video.meta.taskId;videoFileRef=$video.fileRef;videoSha256=(Get-FileHash -LiteralPath $videoPath -Algorithm SHA256).Hash;sourceImage=$spec.imageRef}}
$metadata|ConvertTo-Json -Depth 15|Set-Content -LiteralPath (Join-Path $out "$Character.json") -Encoding utf8
@{character=$Character;taskId=$video.meta.taskId;frames=$frames.frameCount;fps=$frames.fps;video=$videoPath;atlas=(Join-Path $out "$Character.png")} | ConvertTo-Json -Compress
