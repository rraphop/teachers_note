// 데이터 폴더 이름 바꾸기 시험: 예전 폴더(TeacherNotebookV2)에 쌓인 기록이 새 이름(teachers_note)으로 옮겨지는지.
//   가짜 LOCALAPPDATA(임시 폴더)에서 파일 하나 실행기를 켠다. 실제 데이터 폴더·시작 프로그램 목록·켜진 창은 건드리지 않는다
//   (TN_TEST_SANDBOX). 학생·기록은 가짜다.
// 실행: node scripts/e2e/run-datadir.mjs <포터블 TeacherNotebook.exe> <교무수첩_버전.exe> <임시 폴더>
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [appExe, singleExe, sandbox] = process.argv.slice(2);
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "OK " : "NG "} ${name}${detail ? " — " + detail : ""}`);
};
const killUnder = (dir) => {
  try {
    execFileSync("powershell", ["-NoProfile", "-Command", `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -like '${dir.replace(/'/g, "''")}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }`]);
  } catch {}
};
const legacy = path.join(sandbox, "TeacherNotebookV2");
const fresh = path.join(sandbox, "teachers_note");
fs.rmSync(sandbox, { recursive: true, force: true });
fs.mkdirSync(sandbox, { recursive: true });
try {
  // 1. 예전 폴더 이름으로 데이터 만들기 (시험용 데이터 폴더 지정)
  {
    const child = launch(appExe, { dataRoot: legacy });
    const app = await connect(9333);
    await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
    await app.invoke("create_student", { input: { name: "홍길동", class_label: "1-1", number: 1 } });
    await app.invoke("create_note", { raw: "이름 바꾸기 시험 기록" });
    app.close();
    child.kill();
    await sleep(2500);
  }
  check("예전 폴더(TeacherNotebookV2)에 데이터 만듦", fs.existsSync(path.join(legacy, "data", "notebook.db")) && !fs.existsSync(fresh), "");

  // 2. 가짜 LOCALAPPDATA 로 파일 하나 실행기를 켠다 (실제 데이터 폴더 규칙 그대로)
  const env = { ...process.env, LOCALAPPDATA: sandbox, TN_TEST_SANDBOX: "1", TN_TEST_ANSWER: "no", WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9333" };
  delete env.TN_DATA_ROOT;
  const launcher = spawn(singleExe, [], { env, detached: false, stdio: "ignore" });
  const app = await connect(9333, 90000);
  try {
    await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면", timeout: 60000 });
    const info = await app.invoke("app_info");
    const notes = await app.invoke("list_notes", { filter: { limit: 50 } });
    const students = await app.invoke("list_students", { query: null, classId: null });
    check("새 이름 폴더(teachers_note)를 씀", path.resolve(info.data_dir).toLowerCase() === path.resolve(fresh).toLowerCase(), info.data_dir);
    check("예전 폴더는 이름이 바뀌어 남지 않음 (복사본 없음)", !fs.existsSync(legacy), "");
    check("기록·학생 그대로", notes.some((n) => n.raw_text === "이름 바꾸기 시험 기록") && students.some((s) => s.name === "홍길동"), `기록 ${notes.length} 학생 ${students.length}`);
    check("프로그램도 새 폴더에 풀림 (program\\1.0.0)", fs.existsSync(path.join(fresh, "program", info.version, "TeacherNotebook.exe")), "");
    const log = fs.readdirSync(path.join(fresh, "logs")).map((f) => fs.readFileSync(path.join(fresh, "logs", f), "utf8")).join("\n");
    check("실행 기록에 남김 없음 또는 이름 바꿈 기록 (실행기가 먼저 옮김)", !log.includes("바꾸지 못해"), "");
  } finally {
    app.close();
    launcher.kill();
    killUnder(sandbox);
    await sleep(2000);
  }
  // 3. 다시 켜도 새 폴더 그대로 (예전 이름 폴더가 다시 생기지 않음)
  check("다시 볼 때 예전 이름 폴더 없음", !fs.existsSync(legacy) && fs.existsSync(path.join(fresh, "data", "notebook.db")), "");
} catch (e) {
  check("데이터 폴더 시험 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  killUnder(sandbox);
  await sleep(1000);
  fs.rmSync(sandbox, { recursive: true, force: true });
}
const failed = results.filter((r) => !r.ok).length;
console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
process.exit(failed ? 1 : 0);
