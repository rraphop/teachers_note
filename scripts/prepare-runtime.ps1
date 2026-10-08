# llama.cpp 런타임을 src-tauri/runtime/llama.cpp 로 모은다.
# 직접 빌드한 llama.cpp(llvm-mingw, MSVC 런타임 의존 없음, scripts/build-llama.ps1)의 llama-server 와 필요한 DLL만 넣는다.
# GPU(Vulkan) 계산 모듈 ggml-vulkan.dll 도 넣는다 — 그래픽 드라이버의 vulkan-1.dll 이 있는 PC 에서만 읽히고, 없으면 엔진은 CPU 로 돈다.
param(
  [string]$ToolsRoot = (Join-Path (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)) '..\teachers_note-tools'),
  [string]$BuildDir = ''
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$ToolsRoot = (Resolve-Path $ToolsRoot).Path
if (-not $BuildDir) { $BuildDir = Join-Path $ToolsRoot 'build-llama-vk\bin' }
$Dest = Join-Path $Root 'src-tauri\runtime\llama.cpp'
$Mingw = Join-Path $ToolsRoot 'llvm-mingw\x86_64-w64-mingw32\bin'

if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
New-Item -ItemType Directory -Force $Dest | Out-Null

$need = @('llama-server.exe', 'libllama.dll', 'libllama-common.dll', 'libllama-server-impl.dll', 'libmtmd.dll', 'ggml.dll', 'ggml-base.dll')
foreach ($f in $need) {
  $src = Join-Path $BuildDir $f
  if (-not (Test-Path $src)) { throw "빌드 결과에 $f 가 없습니다: $src" }
  Copy-Item $src $Dest
}
# CPU 종류별 계산 모듈 (실행 시 이 PC CPU에 맞는 것을 고른다)
Get-ChildItem $BuildDir -Filter 'ggml-cpu-*.dll' | Copy-Item -Destination $Dest
# GPU 계산 모듈 (그래픽카드가 있으면 엔진이 골라 쓴다)
$vk = Join-Path $BuildDir 'ggml-vulkan.dll'
if (-not (Test-Path $vk)) { throw "GPU 모듈 ggml-vulkan.dll 이 없습니다: $vk (scripts\build-llama.ps1 로 빌드)" }
Copy-Item $vk $Dest

# llvm-mingw C++ 런타임 (Apache-2.0 with LLVM exception)
foreach ($f in @('libc++.dll', 'libunwind.dll')) {
  $src = Join-Path $Mingw $f
  if (-not (Test-Path $src)) { throw "llvm-mingw 런타임 $f 가 없습니다: $src" }
  Copy-Item $src $Dest
}

# 라이선스
Copy-Item (Join-Path $ToolsRoot 'downloads\llama.cpp-LICENSE.txt') (Join-Path $Dest 'LICENSE-llama.cpp.txt')
$llvmLicense = Get-ChildItem (Join-Path $ToolsRoot 'llvm-mingw') -Filter 'LICENSE*' -File | Select-Object -First 1
if ($llvmLicense) { Copy-Item $llvmLicense.FullName (Join-Path $Dest 'LICENSE-llvm-mingw.txt') }

# 필요한 DLL 이 모두 있는지 검사 (시스템 DLL 제외)
$env:PATH = "$(Join-Path $ToolsRoot 'llvm-mingw\bin');$env:PATH"
$present = @(Get-ChildItem $Dest -Filter *.dll | ForEach-Object { $_.Name.ToLower() })
$missing = @()
foreach ($bin in Get-ChildItem $Dest -Include *.exe, *.dll -Recurse) {
  $imports = & llvm-readobj --coff-imports $bin.FullName 2>$null | Select-String 'Name: ' | ForEach-Object { ($_.Line.Trim() -replace 'Name: ', '').ToLower() }
  foreach ($i in $imports) {
    if ($i -match '^(api-ms-|kernel32|advapi32|ws2_32|user32|shell32|ole32|oleaut32|bcrypt|crypt32|ntdll|psapi|iphlpapi|dbghelp|winmm|shlwapi|rpcrt4|version|secur32|ucrtbase|msvcrt|gdi32|comdlg32|setupapi|cfgmgr32|dxgi|d3d|powrprof|userenv|synchronization|bcryptprimitives|vulkan-1)') { continue }
    if ($present -notcontains $i) { $missing += "$($bin.Name) -> $i" }
  }
}
if ($missing.Count -gt 0) { throw "런타임에 없는 DLL: $($missing -join ', ')" }
$bad = @()
foreach ($bin in Get-ChildItem $Dest -Include *.exe, *.dll -Recurse) {
  $imports = & llvm-readobj --coff-imports $bin.FullName 2>$null | Select-String 'Name: ' | ForEach-Object { ($_.Line.Trim() -replace 'Name: ', '').ToLower() }
  if ($imports | Where-Object { $_ -match '^(msvcp140|vcruntime140)' }) { $bad += $bin.Name }
}
if ($bad.Count -gt 0) { throw "Visual C++ 런타임에 의존하는 파일이 있습니다: $($bad -join ', ')" }

Get-ChildItem $Dest | Measure-Object -Property Length -Sum | ForEach-Object { "런타임 준비 완료: 파일 $($_.Count)개, $([math]::Round($_.Sum / 1MB, 1))MB -> $Dest" }
