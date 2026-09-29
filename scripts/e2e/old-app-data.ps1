# 기존 구현 비교 실행 전후로 %LOCALAPPDATA%\TeacherNotebook 를 백업·복원한다 (모델 폴더는 건드리지 않음).
#   백업:  old-app-data.ps1 -Backup
#   복원:  old-app-data.ps1 -Restore -From <백업 폴더>
param([switch]$Backup, [switch]$Restore, [string]$From = '')
$ErrorActionPreference = 'Stop'
$src = Join-Path $env:LOCALAPPDATA 'TeacherNotebook'
$items = @('data', 'backups', 'rules', 'logs', 'webview', 'settings.json')
if (Get-Process TeacherNotebook -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '*dist-portable-final*' }) { throw '기존 앱이 실행 중입니다. 먼저 닫아 주세요.' }

function Hashes($root) {
  Get-ChildItem (Join-Path $root 'data') -File -ErrorAction SilentlyContinue | Sort-Object Name | ForEach-Object { "$($_.Name) $((Get-FileHash $_.FullName).Hash)" }
  if (Test-Path (Join-Path $root 'settings.json')) { "settings.json $((Get-FileHash (Join-Path $root 'settings.json')).Hash)" }
}

if ($Backup) {
  $dest = Join-Path 'C:\Users\rraph\Documents\Claude' ("old-app-backup-" + (Get-Date -Format 'yyyyMMdd-HHmmss'))
  New-Item -ItemType Directory -Force $dest | Out-Null
  foreach ($i in $items) {
    $p = Join-Path $src $i
    if (Test-Path $p -PathType Container) { robocopy $p (Join-Path $dest $i) /E /COPY:DAT /R:1 /W:1 /NFL /NDL /NJH /NJS /NP | Out-Null }
    elseif (Test-Path $p) { Copy-Item $p $dest }
  }
  (Hashes $src) | Out-File -Encoding utf8 (Join-Path $dest 'HASHES-before.txt')
  $cmp = Compare-Object (Hashes $src) (Hashes $dest)
  if ($cmp) { throw "백업 검증 실패: $($cmp | Out-String)" }
  Write-Host "백업 완료: $dest"
}
if ($Restore) {
  if (-not $From -or -not (Test-Path (Join-Path $From 'HASHES-before.txt'))) { throw '복원할 백업 폴더를 지정해 주세요.' }
  foreach ($i in $items) {
    $p = Join-Path $src $i
    $b = Join-Path $From $i
    if (Test-Path $b -PathType Container) {
      robocopy $b $p /MIR /COPY:DAT /R:1 /W:1 /NFL /NDL /NJH /NJS /NP | Out-Null
    } elseif (Test-Path $b) {
      Copy-Item $b $p -Force
    } elseif (Test-Path $p) {
      Remove-Item -Recurse -Force $p
    }
  }
  $before = Get-Content (Join-Path $From 'HASHES-before.txt') -Encoding utf8
  $cmp = Compare-Object $before (Hashes $src)
  if ($cmp) { throw "복원 검증 실패: $($cmp | Out-String)" }
  Write-Host '복원 완료: DB·설정 해시가 백업과 같습니다.'
}
