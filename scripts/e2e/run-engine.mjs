// AI 엔진(llama-server) 정리 E2E: 프로그램을 보통으로 닫을 때와 강제로 끝낼 때(작업 관리자 '작업 끝내기'와 같음)
// 엔진이 남지 않는지, 다시 켜면 엔진이 하나만 뜨는지 확인한다.
// 실행: node scripts/e2e/run-engine.mjs <teachers_note.exe> <데이터폴더> <gguf 경로>
import { execFileSync, execSync } from "node:child_process";
import path from "node:path";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [exe, dataRoot, gguf] = process.argv.slice(2);
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "OK " : "NG "} ${name}${detail ? " — " + detail : ""}`);
};
const ps = (script) => execFileSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).trim();
const pids = (out) => (out ? out.split(/\s+/).map(Number).filter(Boolean) : []);
/** 이 프로그램이 띄운 AI 엔진 (부모가 이 프로그램인 llama-server) */
const enginesOf = (pid) =>
  pids(ps(`Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'llama-server.exe' -and $_.ParentProcessId -eq ${pid} } | ForEach-Object { $_.ProcessId }`));
const alive = (pid) => ps(`if (Get-Process -Id ${pid} -ErrorAction SilentlyContinue) { 'yes' } else { 'no' }`) === "yes";
const exeDir = path.dirname(path.resolve(exe));
/** 이 시험의 프로그램 폴더에서 도는 엔진 전부 (부모가 이미 사라진 고아 포함) */
const enginesUnder = () =>
  pids(ps(`Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'llama-server.exe' -and $_.ExecutablePath -like '${exeDir.replace(/'/g, "''")}*' } | ForEach-Object { $_.ProcessId }`));

async function startEngine(install) {
  const child = launch(exe, { dataRoot });
  const app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  if (install) await app.invoke("import_model_file", { path: gguf });
  const t = Date.now();
  await app.invoke("runtime_start");
  const st = await waitFor(
    async () => {
      const s = await app.invoke("runtime_status");
      return s.state.state === "ready" ? s : null;
    },
    { label: "엔진 켜짐", timeout: 180000, interval: 1000 },
  );
  return { child, app, engines: enginesOf(child.pid), readyPid: st.state.pid, status: st, ms: Date.now() - t };
}

let cur = null;
try {
  // ── 1. 보통으로 닫기 (창 닫기 → 프로그램 끝 → 엔진도 끝) ──
  cur = await startEngine(true);
  check("엔진이 하나 뜸 (프로그램의 자식, 상태의 pid 와 같음)", cur.engines.length === 1 && cur.engines[0] === cur.readyPid, `엔진 ${cur.engines.join(",")} · ${cur.ms}ms`);
  check("엔진은 이 PC 안(127.0.0.1)에서만 연결 받음", cur.status.local_only === true, JSON.stringify(cur.status.listening));
  const e1 = cur.readyPid;
  cur.app.close();
  ps(`(Get-Process -Id ${cur.child.pid}).CloseMainWindow() | Out-Null`);
  const appGone = await waitFor(() => (!alive(cur.child.pid) ? true : null), { label: "프로그램 닫힘", timeout: 20000, interval: 500 }).catch(() => false);
  const engGone = await waitFor(() => (!alive(e1) ? true : null), { label: "엔진 닫힘", timeout: 20000, interval: 500 }).catch(() => false);
  check("창을 닫으면 프로그램과 엔진이 함께 끝남", appGone && engGone, `프로그램 ${appGone ? "끝" : "남음"} · 엔진 ${engGone ? "끝" : "남음"}`);
  cur = null;

  // ── 2. 강제로 끝내기 (작업 관리자 '작업 끝내기' = TerminateProcess) ──
  cur = await startEngine(false);
  check("다시 켜면 엔진이 하나만 뜸 (지난 엔진이 남지 않음)", cur.engines.length === 1 && enginesUnder().length === 1, `엔진 ${cur.engines.join(",")}`);
  const e2 = cur.readyPid;
  cur.app.close();
  execSync(`taskkill /F /PID ${cur.child.pid}`, { stdio: "ignore" });
  const killedEng = await waitFor(() => (!alive(e2) ? true : null), { label: "강제 종료 뒤 엔진", timeout: 15000, interval: 500 }).catch(() => false);
  check("프로그램을 강제로 끝내도 엔진이 남지 않음 (Job Object)", killedEng, killedEng ? "" : `엔진 ${e2} 남음`);
  cur = null;

  // ── 3. 강제 종료 뒤 다시 켜도 정상, [엔진 끄기]는 엔진만 끔 ──
  cur = await startEngine(false);
  check("강제 종료 뒤 다시 켜도 엔진 정상·하나만", cur.engines.length === 1 && enginesUnder().length === 1, `엔진 ${cur.engines.join(",")}`);
  const e3 = cur.readyPid;
  await cur.app.invoke("runtime_stop");
  const stopped = await waitFor(() => (!alive(e3) ? true : null), { label: "엔진 끄기", timeout: 15000, interval: 500 }).catch(() => false);
  check("[엔진 끄기] → 엔진만 끝나고 프로그램은 그대로", stopped && alive(cur.child.pid), "");
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  if (cur) {
    try {
      cur.app.close();
    } catch {}
    cur.child.kill();
  }
  await sleep(1500);
  // 시험이 남긴 엔진이 있으면 정리 (이 시험 프로그램 폴더의 것만)
  for (const pid of enginesUnder()) {
    try {
      execSync(`taskkill /F /PID ${pid}`, { stdio: "ignore" });
    } catch {}
  }
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
