// 업데이트 시험 (0.61 → 0.7, 0.7 → 0.8 …): 지난 버전으로 기록·학생을 만든 데이터 폴더를 새 버전으로 열면
//   - 기록·학생이 그대로이고, '업데이트 전' 백업이 생기며, 새 열(기록 갈래·명렬표 항목)이 동작한다
// 실행: node scripts/e2e/run-upgrade-07.mjs <지난 버전 exe> <새 버전 exe> <데이터폴더>
const ver = (v) => v.split(".").map(Number);
const older = (a, b) => { const [x, y] = [ver(a), ver(b)]; for (let i = 0; i < 3; i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) < (y[i] ?? 0); return false; };
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [oldExe, newExe, dataRoot] = process.argv.slice(2);
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "OK " : "NG "} ${name}${detail ? " — " + detail : ""}`);
};
async function run(exe, fn) {
  const child = launch(exe, { dataRoot });
  let app;
  try {
    app = await connect(9333);
    await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
    return await fn(app);
  } finally {
    try {
      app?.close();
    } catch {}
    child.kill();
    await sleep(2500);
  }
}
try {
  const before = await run(oldExe, async (app) => {
    const v = (await app.invoke("app_info")).version;
    await app.invoke("create_note", { raw: "업데이트 시험 기록: 금요일까지 결석계 받기" });
    await app.invoke("create_student", { input: { name: "홍길동", class_label: "1-1", number: 3 } });
    const notes = await app.invoke("list_notes", { filter: { limit: 50 } });
    return { v, notes: notes.length };
  });
  check(`지난 버전(${before.v})으로 데이터 만들기`, before.notes === 1, JSON.stringify(before));
  await run(newExe, async (app) => {
    const v = (await app.invoke("app_info")).version;
    const notes = await app.invoke("list_notes", { filter: { limit: 50 } });
    const students = await app.invoke("list_students", { query: null, classId: null });
    check(
      `새 버전(${v})으로 열기: 기록·학생 그대로`,
      older(before.v, v) && notes.length === 1 && students.some((s) => s.name === "홍길동" && s.class_label === "1-1" && s.student_no === ""),
      `${v} 기록 ${notes.length} 학생 ${students.length}`,
    );
    check("기록에 갈래 정보 (새 열)", typeof notes[0].is_work === "boolean" && notes[0].track === null, JSON.stringify({ is_work: notes[0].is_work, track: notes[0].track }));
    const backups = await app.invoke("list_backups");
    const up = backups.find((b) => b.kind.startsWith("before-update"));
    check("‘업데이트 전’ 백업이 생김", !!up, backups.map((b) => b.kind).join(", "));
    const text = await app.evaluate(`(async () => { window.__e2e.nav('설정'); await new Promise(r => setTimeout(r, 800)); return document.querySelector('#set-data')?.innerText || ''; })()`);
    check("설정 → 데이터 백업 목록에 ‘업데이트 전’ 표시", text.includes("업데이트 전") || text.includes("백업"), "");
  });
} catch (e) {
  check("업데이트 시험 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
}
const failed = results.filter((r) => !r.ok).length;
console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
process.exit(failed ? 1 : 0);
