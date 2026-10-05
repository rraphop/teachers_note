// 0.6 E2E: 파일 하나 버전 — 실행기로 켜기, 기록 남기기 / (새 버전으로 덮어쓴 뒤) 기록 유지·새 폴더로 풀림·지난 버전 정리
// 실행: node scripts/e2e/run-v06.mjs <실행기 exe> <데이터폴더> <스크린샷폴더> <first|after-update> <기대 버전 폴더>
import fs from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [exe, dataRoot, shots, phase, expectDir] = process.argv.slice(2);
await fs.mkdir(shots, { recursive: true });
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "OK " : "NG "} ${name}${detail ? " — " + detail : ""}`);
};

/** 이 시험 데이터 폴더에서 풀려 실행 중인 교무수첩만 끈다 (교사가 쓰는 창은 건드리지 않음) */
function killTestApps() {
  const prefix = path.join(dataRoot, "program").toLowerCase().replace(/'/g, "''");
  const ps = `Get-CimInstance Win32_Process -Filter "Name='TeacherNotebook.exe'" | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.ToLower().StartsWith('${prefix}') } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force; $_.ProcessId }`;
  try {
    return execFileSync("powershell", ["-NoProfile", "-EncodedCommand", Buffer.from(ps, "utf16le").toString("base64")], { encoding: "utf8" }).trim();
  } catch {
    return "";
  }
}

const child = launch(exe, { dataRoot });
let app;
try {
  app = await connect(9333, 60000);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  const info = await app.invoke("app_info");
  const programDir = path.join(dataRoot, "program", expectDir);
  check("실행기로 켜짐: 파일 위치를 앎", info.launcher_path?.toLowerCase() === path.resolve(exe).toLowerCase(), info.launcher_path ?? "(없음)");
  check(`풀린 위치: program\\${expectDir}`, info.exe_dir.toLowerCase().startsWith(programDir.toLowerCase()), info.exe_dir);
  if (phase === "first") {
    await app.invoke("create_student", { input: { name: "홍길동", class_label: "2-1", number: 3 } });
    await app.invoke("create_note", { raw: "파일 하나 버전 시험 기록: 홍길동 발표를 잘함" });
    await app.nav("설정");
    await sleep(800);
    await app.evaluate(`document.getElementById('set-data').scrollIntoView(); true`);
    await sleep(300);
    const txt = await app.text("#set-data");
    check("설정 → 데이터에 ‘파일 하나 버전’ 위치 표시", txt.includes("파일 하나 버전") && txt.includes(path.basename(exe)), path.basename(exe));
    await app.screenshot(path.join(shots, "58-single-file-settings.png"));
  } else {
    const notes = await app.invoke("list_notes", { filter: { status: "all" } });
    const studs = await app.invoke("list_students", {});
    check("덮어쓴 뒤에도 기록·학생 그대로", notes.some((n) => n.raw_text.includes("파일 하나 버전 시험 기록")) && studs.some((s) => s.name === "홍길동"), `기록 ${notes.length}, 학생 ${studs.length}`);
    await sleep(1500);
    const dirs = existsSync(path.join(dataRoot, "program")) ? (await fs.readdir(path.join(dataRoot, "program"))).filter((d) => !d.startsWith(".")) : [];
    check("지난 버전 폴더는 정리되고 새 버전 폴더만 남음", dirs.length === 1 && dirs[0] === expectDir, dirs.join(","));
  }
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  const killed = killTestApps();
  console.log(`시험용 교무수첩 종료: ${killed || "(없음)"}`);
  await sleep(1500);
  await fs.writeFile(path.join(shots, `results-v06-${phase}.json`), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
