# 여러 모델을 차례로 평가한다. 시험 데이터는 %TEMP% 에 만들고 끝나면 지운다 (모델 복사본 포함).
# 받다 만 모델(.part)이 있으면 이어받는다. 결과 로그: docs\eval\model-evals.log
param([string[]]$Models = @('qwen3.5-2b|download', 'qwen3.5-4b|download', 'qwen3-8b|C:\Users\rraph\AppData\Local\TeacherNotebook\models\Qwen3-8B-Q4_K_M.gguf'))
$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
. (Join-Path (Split-Path -Parent $root) 'teachers_note-tools\env.ps1')
$exe = Join-Path $root 'dist-portable\teachers_note\teachers_note.exe'
$log = Join-Path $root 'docs\eval\model-evals.log'
function Log($m) { $m | Out-File -Append -Encoding utf8 $log; Write-Host $m }
Log "시작 $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
foreach ($m in $Models) {
  $id, $src = $m -split '\|', 2
  $data = Join-Path $env:TEMP "tn-eval-$id"
  # 받던 모델 파일(.part)은 남기고 DB 등은 새로 시작한다
  if (Test-Path $data) { Get-ChildItem $data -Exclude 'models' | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue }
  Log "`n########## $id ($src) ##########"
  $t = Get-Date
  node (Join-Path $root 'scripts\e2e\run-eval.mjs') $exe $data (Join-Path $root 'docs\eval') $id $src 2>&1 | ForEach-Object { Log "$_" }
  Log "소요: $([math]::Round(((Get-Date) - $t).TotalMinutes, 1))분"
  Get-CimInstance Win32_Process | Where-Object { $_.Name -in 'teachers_note.exe', 'llama-server.exe' -and $_.ExecutablePath -like "$root*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Seconds 2
  Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue
}
Log "`n모든 모델 평가 끝 $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
