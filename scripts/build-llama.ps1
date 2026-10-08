# llama.cpp(b11000)를 llvm-mingw 로 빌드한다 — Visual C++ 런타임 의존 없음 (ARCHITECTURE D6, D20).
#  - CPU: 종류별 계산 모듈(GGML_CPU_ALL_VARIANTS)을 실행 시 고른다.
#  - GPU: Vulkan 계산 모듈(ggml-vulkan.dll)을 함께 만든다. 그래픽 드라이버(vulkan-1.dll)가 있는 PC 에서만 읽히고,
#    없거나 실패하면 엔진은 CPU 로 돈다 (GGML_BACKEND_DL).
#  빌드 도구: 도구 폴더의 llvm-mingw·cmake·ninja, msys2(glslc·Vulkan 헤더·vulkan-1 연결 라이브러리)
param(
  [string]$ToolsRoot = (Join-Path (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)) '..\teachers_note-tools'),
  [string]$BuildName = 'build-llama-vk'
)
$ErrorActionPreference = 'Stop'
$ToolsRoot = (Resolve-Path $ToolsRoot).Path
. (Join-Path $ToolsRoot 'env.ps1')
$Src = Join-Path $ToolsRoot 'src\llama.cpp-b11000'
$Build = Join-Path $ToolsRoot $BuildName
$Mingw = Join-Path $ToolsRoot 'llvm-mingw\bin'
$Msys = Join-Path $ToolsRoot 'msys2\mingw64'
$Cmake = Get-ChildItem (Join-Path $ToolsRoot 'cmake') -Recurse -Filter cmake.exe | Select-Object -First 1
$Ninja = Join-Path $ToolsRoot 'ninja\ninja.exe'
# glslc 는 msys2 의 DLL 을 쓰고, 셰이더 생성기는 llvm-mingw 의 libc++ 를 쓴다 (llvm-mingw 를 앞에)
$env:PATH = "$Mingw;$env:PATH;$(Join-Path $Msys 'bin')"
$flags = '-D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00'
# ggml-vulkan.cpp 가 std::function 을 쓰면서 <functional> 을 직접 불러오지 않아 libc++ 에서는 넣어 준다 (소스는 그대로)
# CMake 에 넘기는 경로는 / 로 (역슬래시는 생성 파일에서 이스케이프로 읽힘)
$M = $Mingw.Replace('\', '/'); $S = $Msys.Replace('\', '/'); $N = $Ninja.Replace('\', '/')
# 셰이더 생성기(vulkan-shaders-gen)는 따로 빌드되므로 같은 컴파일러·ninja 를 알려 주는 설정 파일을 만든다
$env:PATH = "$(Split-Path -Parent $Ninja);$env:PATH"
New-Item -ItemType Directory -Force $Build | Out-Null
$Toolchain = Join-Path $Build 'host-toolchain.cmake'
@"
set(CMAKE_C_COMPILER "$M/x86_64-w64-mingw32-clang.exe")
set(CMAKE_CXX_COMPILER "$M/x86_64-w64-mingw32-clang++.exe")
set(CMAKE_RC_COMPILER x86_64-w64-mingw32-windres)
set(CMAKE_MAKE_PROGRAM "$N" CACHE FILEPATH "" FORCE)
"@ | Set-Content -Path $Toolchain -Encoding ascii
& $Cmake.FullName -S $Src -B $Build -G Ninja `
  "-DCMAKE_MAKE_PROGRAM=$N" `
  "-DCMAKE_C_COMPILER=$M/x86_64-w64-mingw32-clang.exe" `
  "-DCMAKE_CXX_COMPILER=$M/x86_64-w64-mingw32-clang++.exe" `
  -DCMAKE_RC_COMPILER=x86_64-w64-mingw32-windres `
  "-DCMAKE_C_FLAGS=$flags" "-DCMAKE_CXX_FLAGS=$flags -include functional" `
  -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON `
  -DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DGGML_CCACHE=OFF `
  -DGGML_VULKAN=ON "-DGGML_VULKAN_SHADERS_GEN_TOOLCHAIN=$($Toolchain.Replace('\', '/'))" `
  "-DCMAKE_PREFIX_PATH=$S" "-DVulkan_INCLUDE_DIR=$S/include" `
  "-DVulkan_LIBRARY=$S/lib/libvulkan-1.dll.a" `
  "-DVulkan_GLSLC_EXECUTABLE=$S/bin/glslc.exe" `
  -DLLAMA_BUILD_SERVER=ON -DLLAMA_BUILD_TOOLS=ON -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF `
  -DLLAMA_BUILD_UI=OFF -DLLAMA_BUILD_MTMD=OFF -DLLAMA_OPENSSL=OFF -DLLAMA_LLGUIDANCE=OFF
if ($LASTEXITCODE -ne 0) { throw 'cmake 설정 실패' }
& $Cmake.FullName --build $Build --config Release -j ([Environment]::ProcessorCount)
if ($LASTEXITCODE -ne 0) { throw 'llama.cpp 빌드 실패' }
Get-ChildItem (Join-Path $Build 'bin') -Filter 'ggml-vulkan.dll' | ForEach-Object { "GPU 모듈: $($_.Name) $([math]::Round($_.Length / 1MB, 1))MB" }
