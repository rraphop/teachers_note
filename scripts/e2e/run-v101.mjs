// 1.0.1 (4차 수정목록) E2E: 선생님 본인 업무(시험감독·보강)는 교무 업무, 학교 일정은 일정만(제목 다듬기),
//   '전학생' 같은 말을 학생으로 보지 않기, 전학(전출·전입) → 명렬표 비고 + 할 일, 문장 안 시각은 한 줄, 버전·업데이트 기록 v1.0.1.
// 학생·학교·기관 이름은 모두 가짜(홍길동·성춘향, OO중)다.
// 실행: node scripts/e2e/run-v101.mjs <teachers_note.exe> <데이터폴더> <스크린샷폴더>
import fs from "node:fs/promises";
import path from "node:path";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [exe, dataRoot, shots] = process.argv.slice(2);
await fs.mkdir(shots, { recursive: true });
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "OK " : "NG "} ${name}${detail ? " — " + detail : ""}`);
};
const pad = (n) => String(n).padStart(2, "0");
const md = (d) => `${d.getMonth() + 1}월 ${d.getDate()}일`;
const today = new Date();

const child = launch(exe, { dataRoot });
let app;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  for (const [c, n, name] of [
    ["2-1", 1, "홍길동"],
    ["2-1", 30, "전학생"], // 예전 빌드가 기록에서 잘못 만든 학생 칸
    ["1-1", 1, "심청"],
  ])
    await app.invoke("create_student", { input: { name, class_label: c, number: n } });
  const studentsNow = async () => Object.fromEntries((await app.invoke("list_students", { query: null, classId: null })).map((s) => [s.name, s]));
  await app.invoke("set_student_field", { id: (await studentsNow())["홍길동"].id, field: "remark", value: "특별반" });
  const classify = async (raw) => {
    const n = await app.invoke("create_note", { raw });
    return app.invoke("save_plan", { id: n.id, plan: await app.invoke("suggest_plan", { id: n.id }) });
  };
  const kinds = (note) => note.chips.map((c) => c.kind);

  // ── 1. 선생님 본인 업무 ──
  let n = await classify("내일 3교시 영어 시험감독");
  check("시험감독 → 교무 업무 + 일정, 학생부 없음", kinds(n).includes("duty") && kinds(n).includes("event") && !kinds(n).includes("student"), JSON.stringify(n.chips));
  n = await classify("3교시 영어 시험감독 중 2-1 학생들 대부분 끝까지 집중함");
  const st = n.chips.filter((c) => c.kind === "student");
  check("시험감독 중 학생들 모습 → 교무 업무 + 학생 미지정 관찰", kinds(n).includes("duty") && st.length === 1 && st[0].label === "학생 미지정", JSON.stringify(n.chips));
  n = await classify("3교시 1-1 심청 영어 시험 중 졸음");
  check("학생이 주어 → 그 학생의 학생부 자료", n.chips.some((c) => c.kind === "student" && c.label.includes("심청")) && !kinds(n).includes("duty"), JSON.stringify(n.chips));

  // ── 2. 학교 일정 ──
  const d2 = new Date(today.getTime() + 2 * 86400000);
  n = await classify(`${d2.getMonth() + 1}.${d2.getDate()}. 15:15 학생자치회 실시 (학생생활협약 조성방안 논의)`);
  check("학생자치회 → 일정만 (‘전학생’ 학생 칸이 있어도 학생부 없음)", kinds(n).join() === "event", JSON.stringify(n.chips));
  check("일정 제목: ‘실시’ 빼기", n.chips[0]?.label === "학생자치회 (학생생활협약 조성방안 논의)", n.chips[0]?.label);
  const lunch = await classify("오늘 교직원 점심은 12:20");
  check("교직원 점심 → 일정만, 제목 ‘교직원 점심 12:20’", kinds(lunch).join() === "event" && lunch.chips[0].label === "교직원 점심 12:20", JSON.stringify(lunch.chips));

  // ── 3. 전학 ──
  const recorded = `(${md(today)} 기록)`;
  n = await classify("2학년 홍길동 OO중 전학예정");
  let s = await studentsNow();
  check("전출 예정 → 학생부 없음", !kinds(n).includes("student"), JSON.stringify(n.chips));
  check("명렬표 비고에 이어 적기", s["홍길동"].remark === `특별반 / OO중 전학 예정 ${recorded}`, s["홍길동"].remark);
  let tasks = await app.invoke("list_tasks", { filter: null });
  check("할 일 ‘홍길동 전출 서류 처리’", tasks.some((t) => t.title === "홍길동 전출 서류 처리" && t.note_id === n.id), tasks.map((t) => t.title).join(", "));
  n = await classify("3학년에 OO중에서 전입생 옴, 이름 성춘향");
  s = await studentsNow();
  check("명렬표에 없는 전입생 → ‘확인 필요’ 학생으로 추가 제안", !!s["성춘향"] && s["성춘향"].needs_review, JSON.stringify(s["성춘향"] ?? null));
  check("전입 비고", s["성춘향"]?.remark === `OO중에서 전입 ${recorded}`, s["성춘향"]?.remark);
  tasks = await app.invoke("list_tasks", { filter: null });
  check("할 일 ‘성춘향 전입 서류 처리’", tasks.some((t) => t.title === "성춘향 전입 서류 처리"), "");
  const hist = await app.invoke("get_student_detail", { id: s["홍길동"].id });
  check("학생부 관찰 없음 · 명렬표 변경 이력에 ‘비고’", hist.observations.length === 0 && hist.profile_history.some((h) => h.field_label === "비고"), "");

  // ── 4. 문장 안 시각은 한 줄 ──
  await app.nav("기록");
  await waitFor(() => app.evaluate(`!!document.getElementById('note-${lunch.id}')`), { label: "기록 카드" });
  const lines = await app.evaluate(`document.querySelectorAll('#note-${lunch.id} .note-raw .tline').length`);
  const rawText = await app.text(`#note-${lunch.id} .note-raw`);
  check("‘점심은 12:20’ 한 줄로 표시", lines === 0 && rawText.trim() === "오늘 교직원 점심은 12:20", `${lines} · ${rawText}`);
  await app.screenshot(path.join(shots, "01-notes.png"));

  // ── 4-2. 두 번째 백업 위치 (다른 드라이브·폴더) ──
  const second = path.join(path.dirname(dataRoot), path.basename(dataRoot) + "-second-drive");
  await fs.rm(second, { recursive: true, force: true });
  await fs.mkdir(second, { recursive: true });
  await app.nav("설정");
  await waitFor(() => app.evaluate(`!!document.querySelector('[data-testid=backup-mirror]')`), { label: "두 번째 백업 칸" });
  await app.evaluate(`window.__tnTestPickPath = ${JSON.stringify(second)}; document.querySelector('[data-testid=backup-mirror]').scrollIntoView(); true`);
  await app.click("폴더 고르기", "[data-testid=backup-mirror] button");
  const mst = await waitFor(async () => {
    const s = await app.invoke("backup_mirror_status");
    return s.dir && s.count >= 1 ? s : null;
  }, { label: "두 번째 위치 복사" });
  const inSecond = async () => (await fs.readdir(path.join(second, "teachers_note_backups"))).filter((f) => f.startsWith("notebook-") && f.endsWith(".db"));
  check("폴더 고르기 → 그 폴더\\teachers_note_backups 에 백업 사본", (await inSecond()).length >= 1 && mst.available, JSON.stringify(mst));
  await sleep(500);
  const box = await app.text("[data-testid=backup-mirror]");
  check("설정 화면에 위치·개수, 같은 드라이브면 안내", box.includes("teachers_note_backups") && box.includes("같은 드라이브"), box.replace(/\n/g, " ").slice(0, 160));
  await app.screenshot(path.join(shots, "03-backup-mirror.png"));
  const before = (await inSecond()).length;
  await app.click("지금 백업", ".row button");
  await waitFor(async () => ((await inSecond()).length > before ? true : null), { label: "지금 백업 → 두 번째 위치" });
  check("[지금 백업] → 두 번째 위치에도 저장", (await inSecond()).length === before + 1, `${before} → ${(await inSecond()).length}`);
  // 다른 설정을 바꿔도 두 번째 위치는 그대로 (예전 화면 값으로 덮어쓰지 않음)
  const cur = await app.invoke("get_settings");
  await app.invoke("update_settings", { value: { ...cur, backup_mirror_dir: null, idle_unload_minutes: 20 } });
  check("다른 설정 저장이 두 번째 위치를 지우지 않음", (await app.invoke("backup_mirror_status")).dir === second, "");
  // 기본 백업이 사라져도(C 드라이브 초기화) 두 번째 위치의 백업으로 되돌릴 수 있다
  const mainDir = path.join(dataRoot, "backups");
  for (const f of (await fs.readdir(mainDir)).filter((f) => f.startsWith("notebook-"))) await fs.rm(path.join(mainDir, f), { force: true });
  const list = await app.invoke("list_backups");
  const mirrorOnly = list.filter((b) => b.location === "mirror");
  check("기본 백업이 없으면 두 번째 위치 백업이 목록에", mirrorOnly.length >= 2, JSON.stringify(list.map((b) => [b.kind, b.location])));
  const counts = await app.invoke("inspect_backup", { name: mirrorOnly[0].name, location: "mirror" });
  check("두 번째 위치 백업 검사(되돌리기 확인 창)", counts.notes >= 1 && counts.students >= 1, JSON.stringify(counts));

  // ── 5. 버전 ──
  const info = await app.invoke("app_info");
  check("버전 1.0.1", info.version === "1.0.1", info.version);
  const btn = await app.text(".update-log-btn");
  check("왼쪽 아래 ‘업데이트 기록 v1.0.1’", btn.includes("v1.0.1"), btn);
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.dialog .changelog')`), { label: "업데이트 기록" });
  const first = await app.text(".changelog > li:first-child");
  check("업데이트 기록 맨 위 v1.0.1 (고친 점·새 기능)", first.includes("v1.0.1") && first.includes("고친 점") && first.includes("새 기능") && first.includes("전학"), first.slice(0, 80));
  const secondEntry = await app.text(".changelog > li:nth-child(2)");
  check("v1.0 기능 소개는 그대로", secondEntry.includes("v1.0") && secondEntry.includes("학생 명렬표"), secondEntry.slice(0, 40));
  await app.screenshot(path.join(shots, "02-changelog.png"));
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  await fs.writeFile(path.join(shots, "results-v101.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
