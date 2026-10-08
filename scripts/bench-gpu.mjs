// AI 엔진 GPU·CPU 속도 재기: 같은 모델·같은 글로 1초에 만드는 토큰 수를 비교한다 (프로그램과 같은 엔진 옵션).
// 실행: node scripts/bench-gpu.mjs <runtime\llama.cpp 폴더> <gguf 경로>
import { spawn } from "node:child_process";
import path from "node:path";

const [dir, gguf] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const prompt =
  "다음 교사 메모를 학생 관찰, 할 일, 일정으로 나눠 JSON 으로 정리하라.\n메모: 오늘 3교시 2-1 홍길동 지각. 금요일까지 보호자 연락. 다음 주 월요일 교직원 회의 자료 준비. 1-2 성춘향 과학 실험 시간에 모둠 친구들에게 실험 순서를 차근차근 설명함.\n답:";

async function run(label, deviceArgs) {
  const port = 18000 + Math.floor(Math.random() * 1000);
  const child = spawn(path.join(dir, "llama-server.exe"), ["--model", gguf, "--host", "127.0.0.1", "--port", String(port), "--ctx-size", "4096", "--parallel", "1", "--reasoning", "off", "--no-webui", "--offline", ...deviceArgs], { cwd: dir, stdio: "ignore" });
  const t0 = Date.now();
  try {
    for (;;) {
      try {
        const r = await fetch(`http://127.0.0.1:${port}/health`);
        if (r.ok) break;
      } catch {}
      if (Date.now() - t0 > 180000) throw new Error("엔진이 켜지지 않음");
      await sleep(300);
    }
    const ready = Date.now() - t0;
    const runs = [];
    for (let i = 0; i < 3; i++) {
      const r = await fetch(`http://127.0.0.1:${port}/completion`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ prompt, n_predict: 200, temperature: 0, cache_prompt: false }) });
      const j = await r.json();
      runs.push({ gen: j.timings.predicted_per_second, prompt: j.timings.prompt_per_second });
    }
    const avg = (k) => runs.reduce((a, x) => a + x[k], 0) / runs.length;
    console.log(`${label}: 켜기 ${(ready / 1000).toFixed(1)}초 · 글 만들기 ${avg("gen").toFixed(1)} 토큰/초 · 입력 읽기 ${avg("prompt").toFixed(0)} 토큰/초`);
    return avg("gen");
  } finally {
    child.kill();
    await sleep(1500);
  }
}

const gpu = await run("GPU (Vulkan0, 모델 전체)", ["--device", "Vulkan0", "--n-gpu-layers", "999"]);
const cpu = await run("CPU (--device none)", ["--device", "none", "--n-gpu-layers", "0", "--threads", "6"]);
console.log(`GPU 가 CPU 보다 ${(gpu / cpu).toFixed(1)}배 빠름`);
