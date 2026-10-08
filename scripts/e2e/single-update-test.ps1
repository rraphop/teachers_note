# 파일 하나 버전 업데이트 시험 ("교무수첩_버전.exe")
#  1) 시험용 '바탕화면'에 빌드 결과(Teachers_Note_1.0.0.exe)를 받는 이름 교무수첩_1.0.0.exe 로 두고 실행 → 기록·학생 남기기
#  2) 새 버전 파일(깃허브에서 받은 영문 이름 Teachers_Note_1.0.1.exe, 시험용)을 옆에 두고 실행 → 기록 유지, 새 폴더로 풀림, 지난 버전 폴더 정리
#     (지난 버전 파일을 휴지통으로 옮길지 묻는 질문에 '아니요')
#  3) 지난 버전 파일을 다시 켜면 → "새 버전을 쓰세요" 질문에 '아니요' → 실행하지 않고 새 버전 폴더도 그대로
#  4) 새 버전 폴더를 지우고 새 파일을 다시 켜면(처음 켜는 상황) → 질문에 '예' → 지난 버전 파일 정리
param([string]$Shots = '')
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
. (Join-Path (Split-Path -Parent $Root) 'teachers_note-tools\env.ps1')
if (-not $Shots) { $Shots = Join-Path $Root 'docs\screenshots' }
$Target = 'x86_64-pc-windows-gnullvm'
$tmp = Join-Path $env:TEMP ("tn-single-e2e-" + [guid]::NewGuid().ToString('N'))
$desk = Join-Path $tmp 'desktop'
$data = Join-Path $tmp 'data'
New-Item -ItemType Directory -Force $desk, $data | Out-Null
$src = Get-ChildItem (Join-Path $Root 'dist-portable') -Filter 'Teachers_Note_*.exe' | Select-Object -First 1
if (-not $src) { throw 'dist-portable 에 Teachers_Note_버전.exe 가 없습니다. 먼저 패키징하세요.' }
$ver = (Get-Content (Join-Path $Root 'src-tauri\Cargo.toml') | Select-String '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$v = $ver.Split('.')
$newVer = "$($v[0]).$($v[1]).$([int]$v[2] + 1)"
$oldExe = Join-Path $desk "교무수첩_$ver.exe"
$newExe = Join-Path $desk "Teachers_Note_$newVer.exe"
$fail = 0
function Run-Node($exe, $phase, $expect) {
  & node (Join-Path $Root 'scripts\e2e\run-v06.mjs') $exe $data $Shots $phase $expect
  if ($LASTEXITCODE -ne 0) { $script:fail++ }
}
try {
  Copy-Item $src.FullName $oldExe
  Write-Host "=== 1. 처음 실행: $($src.Name) ($ver) ==="
  Run-Node $oldExe first $ver

  Write-Host "=== 2. 새 버전 파일 만들기: $(Split-Path $newExe -Leaf) ($newVer, 시험용) ==="
  $work = Join-Path $tmp 'build'
  New-Item -ItemType Directory -Force $work | Out-Null
  $pkg = Join-Path $Root 'dist-portable\teachers_note'
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  # 패키징과 같이 압축하지 않는다 (압축한 묶음은 Windows 보안이 오탐해 시험 파일을 치운다)
  [System.IO.Compression.ZipFile]::CreateFromDirectory($pkg, (Join-Path $work 'payload.zip'), [System.IO.Compression.CompressionLevel]::NoCompression, $false)
  $entries = foreach ($f in (Get-ChildItem $pkg -Recurse -File | Sort-Object FullName)) {
    [ordered]@{ path = $f.FullName.Substring($pkg.Length + 1).Replace('\', '/'); size = $f.Length; sha256 = (Get-FileHash $f.FullName -Algorithm SHA256).Hash.ToLower() }
  }
  $json = [ordered]@{ version = $newVer; main = 'teachers_note.exe'; files = @($entries) } | ConvertTo-Json -Depth 4 -Compress
  [System.IO.File]::WriteAllText((Join-Path $work 'manifest.json'), $json, (New-Object System.Text.UTF8Encoding($false)))
  $env:TN_PAYLOAD = Join-Path $work 'payload.zip'
  $env:TN_MANIFEST = Join-Path $work 'manifest.json'
  $env:CARGO_TARGET_DIR = Join-Path $work 'target'
  Push-Location (Join-Path $Root 'launcher')
  try {
    # cargo 진행 표시는 stderr 로 나오므로 가로채지 않는다 (PowerShell 5.1 은 이를 오류로 보고 멈춘다)
    & cargo build --release --target $Target
    if ($LASTEXITCODE -ne 0) { throw '시험용 새 실행기 빌드 실패' }
  } finally {
    Pop-Location
    Remove-Item Env:\TN_PAYLOAD, Env:\TN_MANIFEST, Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
  }
  Copy-Item (Join-Path $work "target\$Target\release\teachers_note_launcher.exe") $newExe

  Write-Host "=== 3. 새 버전 파일 실행 (지난 파일 정리 질문에 '아니요') ==="
  $env:TN_TEST_ANSWER = 'no'
  Run-Node $newExe after-update $newVer
  if (-not (Test-Path $oldExe)) { Write-Host "NG  '아니요'인데 지난 버전 파일이 사라짐"; $fail++ } else { Write-Host "OK  '아니요' → 지난 버전 파일 그대로" }

  Write-Host "=== 4. 지난 버전 파일을 다시 켬 ('새 버전을 쓰세요' 질문에 '아니요') ==="
  $env:TN_DATA_ROOT = $data
  try {
    $p = Start-Process -FilePath $oldExe -PassThru -Wait -WindowStyle Hidden
  } finally { Remove-Item Env:\TN_DATA_ROOT -ErrorAction SilentlyContinue }
  $dirs = @(Get-ChildItem (Join-Path $data 'program') -Directory | Where-Object { -not $_.Name.StartsWith('.') } | Select-Object -ExpandProperty Name)
  if ($p.ExitCode -eq 3 -and $dirs.Count -eq 1 -and $dirs[0] -eq $newVer) { Write-Host "OK  지난 버전 파일은 실행하지 않고 새 버전 폴더도 그대로 (종료 코드 3, 폴더 $($dirs -join ','))" } else { Write-Host "NG  지난 버전 파일 처리: 종료 코드 $($p.ExitCode), 폴더 $($dirs -join ',')"; $fail++ }

  Write-Host "=== 5. 새 버전을 처음 켜는 상황에서 지난 파일 정리 질문에 '예' ==="
  Remove-Item -Recurse -Force (Join-Path $data "program\$newVer")
  $env:TN_TEST_ANSWER = 'yes'
  Run-Node $newExe after-update $newVer
  if (Test-Path $oldExe) { Write-Host "NG  '예'인데 지난 버전 파일이 남음"; $fail++ } else { Write-Host "OK  '예' → 지난 버전 파일 정리됨, 바탕화면에는 $(Split-Path $newExe -Leaf) 하나" }
} finally {
  Remove-Item Env:\TN_TEST_ANSWER -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
if ($fail -gt 0) { Write-Host "실패 $fail 건" -ForegroundColor Red; exit 1 }
Write-Host '파일 하나 버전 업데이트 시험: 모두 통과' -ForegroundColor Green
