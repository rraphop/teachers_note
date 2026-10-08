# Windows x64 Portable ZIP 만들기
#  1) 프런트엔드 + Release EXE 빌드 (웹 자산은 EXE 안에 내장)
#  2) 폴더 조립: EXE, WebView2Loader.dll, runtime\llama.cpp, resources, licenses, README.txt
#  3) SHA-256 무결성 목록 작성, 금지 파일(DB·GGUF·학생 데이터) 검사
#  4) --self-test 로 내장 화면이 실제로 뜨는지 확인
#  5) ZIP 을 다른 폴더에 풀어 무결성 목록 재검산 + 옮긴 위치에서 --self-test
param(
  [switch]$SkipBuild,
  [string]$OutRoot = ''
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Tools = Join-Path (Split-Path -Parent $Root) 'teachers_note-tools'
. (Join-Path $Tools 'env.ps1')
if (-not $OutRoot) { $OutRoot = Join-Path $Root 'dist-portable' }
$Target = 'x86_64-pc-windows-gnullvm'
$Release = Join-Path $Root "src-tauri\target\$Target\release"
# 프로그램 버전 (tauri.conf.json) — 내려받는 파일 이름에 쓴다
$appVersion = (Get-Content (Join-Path $Root 'src-tauri\tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version
# 빌드 결과 파일 이름 (3차 7번): 영문 Teachers_Note_버전 — 깃허브 릴리즈가 한글 파일 이름을 바꿔 저장하기 때문.
# 사용자가 받아 쓰는 이름(교무수첩_버전.exe)은 릴리즈 표시 이름·홈페이지 내려받기·바탕화면 복사에서 붙인다.
$PkgName = 'teachers_note'
$Pkg = Join-Path $OutRoot $PkgName
$ZipName = "Teachers_Note_${appVersion}_portable.zip"
$Zip = Join-Path $OutRoot $ZipName

function Step($m) { Write-Host "`n=== $m ===" -ForegroundColor Cyan }

Step '1. 빌드'
if (-not $SkipBuild) {
  Push-Location $Root
  try {
    & pnpm tauri build --no-bundle --target $Target
    if ($LASTEXITCODE -ne 0) { throw 'Tauri Release 빌드 실패' }
  } finally { Pop-Location }
}
$exe = Join-Path $Release 'teachers_note.exe'
if (-not (Test-Path $exe)) { throw "EXE 가 없습니다: $exe" }
$runtimeSrc = Join-Path $Root 'src-tauri\runtime\llama.cpp'
if (-not (Test-Path (Join-Path $runtimeSrc 'llama-server.exe'))) { throw 'runtime\llama.cpp 가 준비되지 않았습니다. scripts\prepare-runtime.ps1 을 먼저 실행하세요.' }

Step '2. 폴더 조립'
if (Test-Path $OutRoot) { Remove-Item -Recurse -Force $OutRoot }
New-Item -ItemType Directory -Force $Pkg, "$Pkg\resources", "$Pkg\licenses", "$Pkg\runtime" | Out-Null
Copy-Item $exe $Pkg
Copy-Item (Join-Path $Release 'WebView2Loader.dll') $Pkg
Copy-Item -Recurse $runtimeSrc (Join-Path $Pkg 'runtime\llama.cpp')
Copy-Item (Join-Path $Root 'packaging\README.txt') (Join-Path $Pkg 'README.txt')
# 라이선스 (LICENSE.txt) 와 제3자 고지 (THIRD_PARTY_NOTICES.txt) — 메모장에서 바로 읽히게 BOM 붙은 UTF-8
foreach ($n in 'LICENSE.txt', 'THIRD_PARTY_NOTICES.txt') {
  $txt = [System.IO.File]::ReadAllText((Join-Path $Root $n), [System.Text.Encoding]::UTF8).Replace("`r`n", "`n").Replace("`n", "`r`n")
  [System.IO.File]::WriteAllText((Join-Path $Pkg $n), $txt, (New-Object System.Text.UTF8Encoding($true)))
}

# 모델 카탈로그 (주소·크기·SHA-256·라이선스) 를 투명하게 공개
& $exe --export-catalog (Join-Path $Pkg 'resources\model-catalog.json')
Start-Sleep -Milliseconds 300
if (-not (Test-Path (Join-Path $Pkg 'resources\model-catalog.json'))) { throw '모델 카탈로그를 내보내지 못했습니다' }

Step '3. 제3자 라이선스'
$lic = Join-Path $Pkg 'licenses'
$notice = New-Object System.Collections.Generic.List[string]
$notice.Add('로컬 AI 교무수첩 — 제3자 구성요소 고지 (Third-Party Notices)')
$notice.Add('')
$notice.Add('[AI 엔진] llama.cpp / ggml b11000 — MIT License — https://github.com/ggml-org/llama.cpp')
$notice.Add('  이 배포본의 runtime\llama.cpp 는 공식 소스(태그 b11000)를 llvm-mingw(clang 23)로 직접 빌드한 것입니다.')
$notice.Add('[C++ 런타임] LLVM libc++ / libunwind (llvm-mingw) — Apache-2.0 WITH LLVM-exception — https://github.com/mstorsjo/llvm-mingw')
$notice.Add('[화면] Microsoft Edge WebView2 Loader (WebView2Loader.dll) — Microsoft WebView2 SDK 라이선스에 따른 재배포 — https://aka.ms/webview2')
$notice.Add('[데이터베이스] SQLite — Public Domain — https://sqlite.org')
$notice.Add('[AI 모델] 모델 파일은 이 배포본에 들어 있지 않습니다. 사용자가 설정에서 받을 때 각 모델의 라이선스(resources\model-catalog.json)를 따릅니다.')
$notice.Add('')
Copy-Item (Join-Path $runtimeSrc 'LICENSE-llama.cpp.txt') (Join-Path $lic 'llama.cpp-MIT.txt')
if (Test-Path (Join-Path $runtimeSrc 'LICENSE-llvm-mingw.txt')) { Copy-Item (Join-Path $runtimeSrc 'LICENSE-llvm-mingw.txt') (Join-Path $lic 'llvm-mingw.txt') }

# Rust 크레이트
$meta = & cargo metadata --format-version 1 --manifest-path (Join-Path $Root 'src-tauri\Cargo.toml') --filter-platform $Target | ConvertFrom-Json
$ids = New-Object System.Collections.Generic.HashSet[string]
foreach ($n in $meta.resolve.nodes) { [void]$ids.Add($n.id) }
$notice.Add('[Rust 크레이트]')
$crateDir = Join-Path $lic 'rust'
New-Item -ItemType Directory -Force $crateDir | Out-Null
foreach ($p in ($meta.packages | Sort-Object name)) {
  if (-not $ids.Contains($p.id)) { continue }
  if ($p.name -eq 'teachers_note') { continue }
  $licName = if ($p.license) { $p.license } else { '(license-file)' }
  $notice.Add("  $($p.name) $($p.version) — $licName$(if ($p.repository) { ' — ' + $p.repository })")
  $dir = Split-Path -Parent $p.manifest_path
  $files = Get-ChildItem $dir -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)' }
  if ($files) {
    $d = Join-Path $crateDir "$($p.name)-$($p.version)"
    New-Item -ItemType Directory -Force $d | Out-Null
    $files | Copy-Item -Destination $d
  }
}
# 화면(npm) 패키지
$notice.Add('')
$notice.Add('[화면 라이브러리 (npm)]')
$npmDir = Join-Path $lic 'npm'
New-Item -ItemType Directory -Force $npmDir | Out-Null
$pkgJson = Get-Content (Join-Path $Root 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$npmNames = @($pkgJson.dependencies.PSObject.Properties.Name) + @('scheduler')
foreach ($name in $npmNames) {
  $dir = Join-Path $Root "node_modules\$name"
  if ($name -eq 'scheduler') { $dir = (Get-ChildItem (Join-Path $Root 'node_modules\.pnpm') -Directory -Filter 'scheduler@*' | Select-Object -First 1 | ForEach-Object { Join-Path $_.FullName 'node_modules\scheduler' }) }
  if (-not $dir -or -not (Test-Path $dir)) { continue }
  $pj = Get-Content (Join-Path $dir 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json
  $notice.Add("  $name $($pj.version) — $($pj.license)")
  $files = Get-ChildItem $dir -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE)' }
  if ($files) {
    $d = Join-Path $npmDir ($name -replace '[@/]', '_')
    New-Item -ItemType Directory -Force $d | Out-Null
    $files | Copy-Item -Destination $d
  }
}
[System.IO.File]::WriteAllLines((Join-Path $lic 'THIRD-PARTY-NOTICES.txt'), $notice, (New-Object System.Text.UTF8Encoding($true)))

Step '4. 금지 파일 검사'
$forbidden = Get-ChildItem $Pkg -Recurse -File | Where-Object { $_.Extension -in '.db', '.sqlite', '.sqlite3', '.gguf', '.part', '.db-wal', '.db-shm' -or $_.Name -in 'settings.json', 'notebook.db' }
if ($forbidden) { throw "배포본에 들어가면 안 되는 파일: $($forbidden.FullName -join ', ')" }
$bigText = Get-ChildItem $Pkg -Recurse -File -Include *.txt, *.json | Select-String -Pattern '홍길동|김민수|학생부 원문' -List
if ($bigText) { throw "배포본 텍스트에 학생 예시 데이터가 들어 있습니다: $($bigText.Path -join ', ')" }

Step '5. SHA-256 무결성 목록'
$sumsPath = Join-Path $Pkg 'SHA256SUMS.txt'
$lines = New-Object System.Collections.Generic.List[string]
foreach ($f in (Get-ChildItem $Pkg -Recurse -File | Sort-Object FullName)) {
  if ($f.FullName -eq $sumsPath) { continue }
  $rel = $f.FullName.Substring($Pkg.Length + 1)
  $h = (Get-FileHash $f.FullName -Algorithm SHA256).Hash.ToLower()
  $lines.Add("$h  $rel")
}
[System.IO.File]::WriteAllLines($sumsPath, $lines, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "파일 $($lines.Count)개"

function Invoke-SelfTest($dir, $label) {
  $dataRoot = Join-Path $env:TEMP ("tn-selftest-" + [guid]::NewGuid().ToString('N'))
  $report = Join-Path $dataRoot 'report.json'
  New-Item -ItemType Directory -Force $dataRoot | Out-Null
  $env:TN_DATA_ROOT = $dataRoot
  try {
    $p = Start-Process -FilePath (Join-Path $dir 'teachers_note.exe') -ArgumentList @('--self-test', $report) -PassThru -WindowStyle Hidden
    if (-not $p.WaitForExit(90000)) { $p.Kill(); throw "${label}: self-test 시간 초과" }
    if (-not (Test-Path $report)) { throw "${label}: self-test 보고서가 없습니다 (종료 코드 $($p.ExitCode))" }
    $r = Get-Content $report -Raw -Encoding UTF8 | ConvertFrom-Json
    Write-Host "${label} self-test: ok=$($r.ok) assets=$($r.assets_embedded) frontend=$($r.frontend_loaded) runtime=$($r.runtime_found) fts5=$($r.fts5_ok) exe_dir=$($r.exe_dir)"
    if (-not $r.ok -or -not $r.runtime_found) { throw "${label}: self-test 실패" }
    if (-not $r.runtime_dir.StartsWith($dir)) { throw "${label}: 런타임을 실행 파일 위치가 아닌 곳에서 찾았습니다: $($r.runtime_dir)" }
    return $r
  } finally {
    Remove-Item Env:\TN_DATA_ROOT -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $dataRoot -ErrorAction SilentlyContinue
  }
}

Step '6. self-test (조립한 폴더)'
$null = Invoke-SelfTest $Pkg '조립 폴더'

Step '7. ZIP 만들기'
if (Test-Path $Zip) { Remove-Item -Force $Zip }
& tar -a -c -f $Zip -C $OutRoot $PkgName
if ($LASTEXITCODE -ne 0) { throw 'ZIP 만들기 실패' }
$zipHash = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
[System.IO.File]::WriteAllText((Join-Path $OutRoot "$ZipName.sha256"), "$zipHash  $ZipName`r`n", (New-Object System.Text.UTF8Encoding($false)))
Write-Host "ZIP: $Zip ($([math]::Round((Get-Item $Zip).Length / 1MB, 1))MB) sha256=$zipHash"

Step '8. 다른 폴더에 풀어서 재검산 + 옮긴 위치 self-test'
$moved = Join-Path $env:TEMP ("tn-moved-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $moved | Out-Null
try {
  & tar -x -f $Zip -C $moved
  $mpkg = Join-Path $moved $PkgName
  $bad = 0
  foreach ($line in Get-Content (Join-Path $mpkg 'SHA256SUMS.txt') -Encoding UTF8) {
    if (-not $line.Trim()) { continue }
    $h, $rel = $line -split '  ', 2
    $actual = (Get-FileHash (Join-Path $mpkg $rel) -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $h) { Write-Host "불일치: $rel" -ForegroundColor Red; $bad++ }
  }
  if ($bad -gt 0) { throw "무결성 목록 불일치 $bad 개" }
  Write-Host '무결성 목록 재검산: 모두 일치'
  $null = Invoke-SelfTest $mpkg '옮긴 폴더'
} finally {
  Remove-Item -Recurse -Force $moved -ErrorAction SilentlyContinue
}

Step '9. 파일 하나 버전 Teachers_Note_버전.exe (받는 이름 교무수첩_버전.exe)'
# 조립한 폴더 전체를 묶음(payload.zip)으로 만들고, 파일 목록과 SHA-256 을 실행기 안에 함께 넣는다
# (버전은 맨 위에서 tauri.conf.json 으로 읽었다)
$work = Join-Path $OutRoot '.single-work'
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Force $work | Out-Null
$payload = Join-Path $work 'payload.zip'
Add-Type -AssemblyName System.IO.Compression.FileSystem
# 묶음은 압축하지 않는다: 압축한 큰 묶음을 품은 서명 없는 실행기를 Windows 보안(Defender)의 기계학습 판정이
# 트로이목마(Trojan:Win32/Sabsik.FL.A!ml)로 오탐했다 (1.0, 압축 32MB → 탐지 / 압축 안 함 85MB → 깨끗)
[System.IO.Compression.ZipFile]::CreateFromDirectory($Pkg, $payload, [System.IO.Compression.CompressionLevel]::NoCompression, $false)
$entries = foreach ($f in (Get-ChildItem $Pkg -Recurse -File | Sort-Object FullName)) {
  [ordered]@{ path = $f.FullName.Substring($Pkg.Length + 1).Replace('\', '/'); size = $f.Length; sha256 = (Get-FileHash $f.FullName -Algorithm SHA256).Hash.ToLower() }
}
$manifestPath = Join-Path $work 'manifest.json'
$manifestJson = [ordered]@{ version = $appVersion; main = 'teachers_note.exe'; files = @($entries) } | ConvertTo-Json -Depth 4 -Compress
[System.IO.File]::WriteAllText($manifestPath, $manifestJson, (New-Object System.Text.UTF8Encoding($false)))
$env:TN_PAYLOAD = $payload
$env:TN_MANIFEST = $manifestPath
# cargo 는 현재 폴더에서 .cargo\config.toml 을 찾으므로 실행기 폴더에서 빌드한다
Push-Location (Join-Path $Root 'launcher')
try {
  & cargo build --release --target $Target
  if ($LASTEXITCODE -ne 0) { throw '실행기 빌드 실패' }
} finally {
  Pop-Location
  Remove-Item Env:\TN_PAYLOAD, Env:\TN_MANIFEST -ErrorAction SilentlyContinue
}
# 파일 이름: Teachers_Note_버전.exe — 프로그램 버전 그대로 (예: Teachers_Note_1.0.0.exe)
$singleName = "Teachers_Note_$appVersion.exe"
$single = Join-Path $OutRoot $singleName
Copy-Item (Join-Path $Root "launcher\target\$Target\release\teachers_note_launcher.exe") $single -Force
Remove-Item -Recurse -Force $work
$singleHash = (Get-FileHash $single -Algorithm SHA256).Hash.ToLower()
[System.IO.File]::WriteAllText((Join-Path $OutRoot "$singleName.sha256"), "$singleHash  $singleName`r`n", (New-Object System.Text.UTF8Encoding($false)))
Write-Host "파일 하나: $single ($([math]::Round((Get-Item $single).Length / 1MB, 1))MB) sha256=$singleHash"

function Invoke-SingleTest($dataRoot, $label) {
  $report = Join-Path $dataRoot "report-$([guid]::NewGuid().ToString('N')).json"
  $env:TN_DATA_ROOT = $dataRoot
  try {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath $single -ArgumentList @('--self-test', $report) -PassThru -WindowStyle Hidden
    if (-not $p.WaitForExit(120000)) { $p.Kill(); throw "${label}: 시간 초과" }
    $sw.Stop()
    if (-not (Test-Path $report)) { throw "${label}: self-test 보고서가 없습니다 (종료 코드 $($p.ExitCode))" }
    $r = Get-Content $report -Raw -Encoding UTF8 | ConvertFrom-Json
    $want = Join-Path $dataRoot "program\$appVersion"
    Write-Host "${label}: ok=$($r.ok) runtime=$($r.runtime_found) $([math]::Round($sw.Elapsed.TotalSeconds, 1))초 exe_dir=$($r.exe_dir)"
    if (-not $r.ok -or -not $r.runtime_found) { throw "${label}: self-test 실패" }
    if (-not $r.exe_dir.StartsWith($want)) { throw "${label}: 풀린 위치가 다릅니다: $($r.exe_dir)" }
  } finally {
    Remove-Item Env:\TN_DATA_ROOT -ErrorAction SilentlyContinue
  }
}
$sroot = Join-Path $env:TEMP ("tn-single-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $sroot | Out-Null
try {
  Invoke-SingleTest $sroot '처음 실행(풀기)'
  Invoke-SingleTest $sroot '두 번째 실행(검사만)'
  # 풀린 파일이 망가지면 다시 푼다
  Remove-Item (Join-Path $sroot "program\$appVersion\WebView2Loader.dll")
  Set-Content -Path (Join-Path $sroot "program\$appVersion\README.txt") -Value 'tampered' -Encoding ascii
  Invoke-SingleTest $sroot '손상 뒤 다시 풀기'
  # 지난 버전 폴더는 정리한다
  $old = Join-Path $sroot 'program\0.0.1'
  New-Item -ItemType Directory -Force $old | Out-Null
  Invoke-SingleTest $sroot '지난 버전 정리'
  if (Test-Path $old) { throw '지난 버전 폴더를 정리하지 못했습니다' }
  Write-Host '파일 하나 버전 검사: 모두 통과'
} finally {
  Remove-Item -Recurse -Force $sroot -ErrorAction SilentlyContinue
}
Step '10. Windows 보안(Defender) 검사 — 격리하지 않고 검사만'
# 내려받은 사람의 PC에서 막히지 않는지 미리 본다. 탐지되면 배포하지 말고 원인을 찾는다.
$mp = Join-Path $env:ProgramFiles 'Windows Defender\MpCmdRun.exe'
if (Test-Path $mp) {
  foreach ($f in @($single, $Zip, (Join-Path $Pkg 'teachers_note.exe'))) {
    $out = & $mp -Scan -ScanType 3 -File $f -DisableRemediation 2>&1 | Out-String
    if ($LASTEXITCODE -eq 2) {
      $name = (($out -split "`n") | Where-Object { $_ -match '^Threat\s' }) -join ' '
      throw "Windows 보안이 $(Split-Path $f -Leaf) 을(를) 위협으로 판정했습니다: $($name.Trim())"
    }
    Write-Host "깨끗함: $(Split-Path $f -Leaf)"
  }
} else {
  Write-Warning 'Windows 보안 검사 도구(MpCmdRun.exe)가 없어 검사를 건너뜁니다.'
}

Write-Host "`n완료: $Zip`n      $single" -ForegroundColor Green
