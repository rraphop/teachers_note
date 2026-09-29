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
$Tools = Join-Path (Split-Path -Parent $Root) 'teacher-notebook-v2-tools'
. (Join-Path $Tools 'env.ps1')
if (-not $OutRoot) { $OutRoot = Join-Path $Root 'dist-portable' }
$Target = 'x86_64-pc-windows-gnullvm'
$Release = Join-Path $Root "src-tauri\target\$Target\release"
$PkgName = 'TeacherNotebook-Portable'
$Pkg = Join-Path $OutRoot $PkgName
$Zip = Join-Path $OutRoot 'TeacherNotebook-Portable-x64.zip'

function Step($m) { Write-Host "`n=== $m ===" -ForegroundColor Cyan }

Step '1. 빌드'
if (-not $SkipBuild) {
  Push-Location $Root
  try {
    & pnpm tauri build --no-bundle --target $Target
    if ($LASTEXITCODE -ne 0) { throw 'Tauri Release 빌드 실패' }
  } finally { Pop-Location }
}
$exe = Join-Path $Release 'TeacherNotebook.exe'
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
  if ($p.name -eq 'teacher-notebook') { continue }
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
$bigText = Get-ChildItem $Pkg -Recurse -File -Include *.txt, *.json | Select-String -Pattern '김민수|학생부 원문' -List
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
    $p = Start-Process -FilePath (Join-Path $dir 'TeacherNotebook.exe') -ArgumentList @('--self-test', $report) -PassThru -WindowStyle Hidden
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
"$zipHash  TeacherNotebook-Portable-x64.zip" | Out-File -Encoding ascii (Join-Path $OutRoot 'TeacherNotebook-Portable-x64.zip.sha256')
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
Write-Host "`n완료: $Zip" -ForegroundColor Green
