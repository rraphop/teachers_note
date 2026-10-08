// 데이터 형식 업데이트 시험 (1.0 첫 빌드: 형식 8 → 1.0 2차 수정: 형식 9)
//   지난 빌드로 기록·학생(보호자 연락처 한 칸)·할 일·메모를 만든 데이터 폴더를 새 빌드로 열면
//   - 모두 그대로이고 '업데이트 전' 백업이 생기며, 보호자 연락처가 연락처 목록 첫 번째로 옮겨진다
//   - 업데이트 전에 있던 항목은 '새 항목' 숫자로 세지 않는다
// 학생·번호는 가짜다.
// 실행: node scripts/e2e/run-upgrade-10.mjs <지난 빌드 exe> <새 빌드 exe> <데이터폴더>
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
    const sid = await app.invoke("create_student", { input: { name: "홍길동", class_label: "1-1", number: 3 } });
    await app.invoke("set_student_field", { id: sid, field: "guardian_phone", value: "010-9876-5432" });
    await app.invoke("create_note", { raw: "업데이트 시험 기록: 금요일까지 결석계 받기" });
    await app.invoke("create_task", { input: { title: "업데이트 시험 할 일", due_date: null } });
    const memo = await app.invoke("create_memo", { color: null });
    await app.invoke("update_memo", { id: memo.id, body: "업데이트 시험 메모", color: null });
    const backups = await app.invoke("list_backups");
    return { v: (await app.invoke("app_info")).version, backups: backups.length, sid };
  });
  check(`지난 빌드(${before.v})로 데이터 만들기`, !!before.sid, JSON.stringify(before));
  await run(newExe, async (app) => {
    const v = (await app.invoke("app_info")).version;
    const notes = await app.invoke("list_notes", { filter: { limit: 50 } });
    const students = await app.invoke("list_students", { query: null, classId: null });
    const tasks = await app.invoke("list_tasks", { state: "open", dutyId: null });
    const memos = await app.invoke("list_memos");
    const s = students.find((x) => x.name === "홍길동");
    check(`새 빌드(${v})로 열기: 기록·학생·할 일·메모 그대로`, notes.length === 1 && !!s && tasks.some((t) => t.title === "업데이트 시험 할 일") && memos.length === 1 && memos[0].body === "업데이트 시험 메모", `기록 ${notes.length} 학생 ${students.length} 할 일 ${tasks.length} 메모 ${memos.length}`);
    check(
      "보호자 연락처 한 칸 → 연락처 목록 첫 번째 (예전 칸도 그대로)",
      s && s.contacts.length === 1 && s.contacts[0].phone === "010-9876-5432" && s.guardian_phone === "010-9876-5432",
      JSON.stringify(s?.contacts),
    );
    const backups = await app.invoke("list_backups");
    const up = backups.filter((b) => b.kind.startsWith("before-update"));
    check("‘업데이트 전’ 백업이 생김 (지난 빌드의 데이터 형식에서)", up.length > 0, backups.map((b) => b.kind).join(", "));
    const ov = await app.invoke("today_overview");
    check("업데이트 전 항목은 새 항목으로 세지 않음", JSON.stringify(ov.new_counts) === JSON.stringify({ records: 0, duties: 0, tasks: 0, students: 0 }), JSON.stringify(ov.new_counts));
    await app.nav("학생 명렬표");
    await sleep(800);
    check("왼쪽 메뉴 ‘학생 명렬표’로 열림", await app.evaluate(`!!document.querySelector('.student-search')`), "");
  });
} catch (e) {
  check("업데이트 시험 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
}
const failed = results.filter((r) => !r.ok).length;
console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
process.exit(failed ? 1 : 0);
