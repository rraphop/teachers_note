// E2E 1단계: AI 모델 없이 (시나리오 E) — 원문 저장, 미분류 이유, 직접 분류, 일정표 반영, 담당업무
// 실행: node scripts/e2e/run-noai.mjs <teachers_note.exe> <데이터폴더> <스크린샷폴더>
import fs from "node:fs/promises";
import path from "node:path";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [exe, dataRoot, shots] = process.argv.slice(2);
await fs.mkdir(shots, { recursive: true });
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "✔" : "✘"} ${name}${detail ? " — " + detail : ""}`);
};

// '금요일까지'는 오늘부터 가장 가까운 금요일 (시험을 돌리는 날에 따라 달라진다)
const pad = (n) => String(n).padStart(2, "0");
const iso = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const now0 = new Date();
const fri = new Date(now0.getFullYear(), now0.getMonth(), now0.getDate() + ((5 - now0.getDay() + 7) % 7));
const FRI = iso(fri);
const DAY_BEFORE_FRI = iso(new Date(fri.getFullYear(), fri.getMonth(), fri.getDate() - 1));
const FRI_MON = iso(new Date(fri.getFullYear(), fri.getMonth(), fri.getDate() - 4));
const FRI_SUN = iso(new Date(fri.getFullYear(), fri.getMonth(), fri.getDate() + 2));

const child = launch(exe, { dataRoot });
let app;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  await sleep(800);
  await app.screenshot(path.join(shots, "01-today-first-run.png"));
  check("첫 화면에서 바로 기록 입력창이 보임", await app.evaluate(`!!document.querySelector('.capture textarea')`));
  check("AI 모델 없음 안내가 보임", (await app.text()).includes("AI 모델이 아직 없습니다"));

  // 시나리오 E + A 원문: 모델 없이 기록
  const A = "오늘 3교시 2-1 홍길동 지각. 담임에게 알리고 금요일까지 보호자 연락.";
  const t0 = Date.now();
  await app.type(".capture textarea", A);
  await app.click("기록", ".capture button");
  const saved = await waitFor(async () => {
    const notes = await app.invoke("list_notes", { filter: { limit: 5 } });
    return notes.find((n) => n.raw_text === A);
  }, { label: "원문 저장" });
  check("원문 즉시 저장", !!saved, `${Date.now() - t0}ms`);
  const failed = await waitFor(async () => {
    const d = await app.invoke("get_note_detail", { id: saved.id });
    return d.note.status === "failed" ? d.note : null;
  }, { label: "미분류 상태" });
  check("모델 없을 때 미분류 상태와 이유 표시", failed.status_reason?.includes("AI 모델이 아직"), failed.status_reason);
  await sleep(500);
  await app.screenshot(path.join(shots, "02-note-unclassified.png"));

  // 직접 분류: 규칙 제안이 미리 채워진다
  await app.click("직접 분류");
  await waitFor(() => app.evaluate(`!!document.querySelector('.dialog .plan-item')`), { label: "직접 분류 창" });
  await sleep(400);
  await app.screenshot(path.join(shots, "03-manual-classify-prefilled.png"));
  const dialogText = await app.text(".dialog");
  check("직접 분류 창에 규칙 제안(학생 관찰·할 일) 채워짐", dialogText.includes("학생 관찰") && dialogText.includes("보호자 연락"));
  await app.click("저장", ".dialog-foot button");
  const manual = await waitFor(async () => {
    const d = await app.invoke("get_note_detail", { id: saved.id });
    return d.note.status === "manual" ? d : null;
  }, { label: "직접 분류 저장" });
  const chips = manual.note.chips;
  check("학생 2-1 홍길동 연결 + 출결 영역", chips.some((c) => c.kind === "student" && c.label === "2-1 홍길동" && c.area === "attendance"), JSON.stringify(chips));
  const task = chips.find((c) => c.kind === "task" && c.label === "보호자 연락");
  check(`보호자 연락 = 가까운 금요일(${FRI}) 기한 할 일`, task?.date === FRI, JSON.stringify(task));
  const tasks = await app.invoke("list_tasks", { filter: { state: "open" } });
  const contact = tasks.find((t) => t.title === "보호자 연락");
  // 1일 전 08:30 이 이미 지났으면(시험을 그 뒤에 돌린 경우) 다음 알림이 없어야 한다
  const remindAt = `${DAY_BEFORE_FRI}T08:30:00`;
  const passed = new Date() >= new Date(remindAt);
  check("할 일에 알림 예약(1일 전 08:30)", passed ? !contact?.next_reminder : contact?.next_reminder === remindAt, `${contact?.next_reminder ?? "없음"}${passed ? " (예약 시각이 이미 지남)" : ""}`);
  await sleep(500);
  await app.screenshot(path.join(shots, "04-note-manual-classified.png"));

  // 시나리오 B (모델 없이 규칙 제안만) — 출결로 분류하지 않음
  const B = "학생들이 수업 중 손장난을 했고 지적을 받았습니다.";
  const nb = await app.invoke("create_note", { raw: B });
  const planB = await app.invoke("suggest_plan", { id: nb.id });
  check("시나리오 B 규칙 제안: 출결 아님, 교과", planB.items.every((i) => i.area !== "attendance") && planB.items.some((i) => i.area === "subject"), JSON.stringify(planB.items.map((i) => [i.kind, i.area])));
  check("시나리오 B 규칙 제안: 할 일 없음", planB.items.every((i) => i.kind !== "task"));

  // 시나리오 C — 해석 없이 일정
  const C = "10월 15일 교육장 공감소통 방문";
  const nc = await app.invoke("create_note", { raw: C });
  const planC = await app.invoke("suggest_plan", { id: nc.id });
  const ev = planC.items.find((i) => i.kind === "event");
  check("시나리오 C: 일정, 꾸밈 없음, 10/15", ev?.text === "교육장 공감소통 방문" && ev?.date === "2026-10-15", JSON.stringify(ev));
  await app.invoke("save_plan", { id: nc.id, plan: planC });

  // 시나리오 F — 일정 반영 방향
  await app.invoke("create_event", { input: { layer: "annual", title: "2학기 중간고사", start_date: "2026-10-14", end_date: "2026-10-16", time: null, date_expr: null, memo: null } });
  await app.invoke("create_event", { input: { layer: "monthly", title: "교과협의회", start_date: "2026-10-13", end_date: null, time: "15:30", date_expr: null, memo: null } });
  await app.invoke("create_event", { input: { layer: "weekly", title: "3교시 공개수업 준비", start_date: "2026-10-12", end_date: null, time: null, date_expr: null, memo: null } });
  const titles = async (view, s, e) => (await app.invoke("calendar", { view, start: s, end: e })).map((i) => i.title);
  const yearT = await titles("year", "2026-01-01", "2026-12-31");
  const monthT = await titles("month", "2026-09-28", "2026-11-08");
  const weekT = await titles("week", "2026-10-12", "2026-10-18");
  check("연간: 학사력만 (월간·주간·기록 일정 없음)", yearT.includes("2학기 중간고사") && !yearT.includes("교과협의회") && !yearT.includes("3교시 공개수업 준비") && !yearT.includes("교육장 공감소통 방문"), yearT.join(","));
  check("월간: 학사력+월간+기록 일정 (주간 계획 없음)", monthT.includes("2학기 중간고사") && monthT.includes("교과협의회") && monthT.includes("교육장 공감소통 방문") && !monthT.includes("3교시 공개수업 준비"), monthT.join(","));
  check("주간: 모두 표시", ["2학기 중간고사", "교과협의회", "3교시 공개수업 준비", "교육장 공감소통 방문"].every((x) => weekT.includes(x)), weekT.join(","));
  const weekOf = await titles("week", FRI_MON, FRI_SUN);
  check("날짜 있는 할 일이 주간 달력에 표시", weekOf.includes("보호자 연락"), weekOf.join(","));

  // 일정표 화면 캡처 (주간: 10/12 주)
  await app.nav("일정표");
  await waitFor(() => app.evaluate(`!!document.querySelector('.week-grid')`), { label: "주간 표" });
  await app.evaluate(`window.__e2e.type('.cal-nav input[type=date]', '2026-10-14')`);
  await sleep(700);
  await app.screenshot(path.join(shots, "05-calendar-week.png"));
  await app.click("월간", ".seg button");
  await sleep(700);
  await app.screenshot(path.join(shots, "06-calendar-month.png"));
  await app.click("학사력", ".seg button");
  await sleep(700);
  await app.screenshot(path.join(shots, "07-calendar-year.png"));

  // 시나리오 D — 담당업무 (단순 사실은 할 일로 만들지 않음)
  await app.invoke("create_duty", { input: { name: "교무기획", detail: "학사일정 관리", guide: "학사력 작성·변경, 수업일수, 교육청 학사일정 보고", keywords: "학사력, 수업일수", active: true } });
  const D1 = "2학기 학사일정 변경안이 교무회의에서 확정됨.";
  const nd1 = await app.invoke("create_note", { raw: D1 });
  const planD1 = await app.invoke("suggest_plan", { id: nd1.id });
  check("시나리오 D: 업무 기록만, 할 일 없음", planD1.items.some((i) => i.kind === "duty") && planD1.items.every((i) => i.kind !== "task"), JSON.stringify(planD1.items.map((i) => i.kind)));
  await app.invoke("save_plan", { id: nd1.id, plan: planD1 });
  const D2 = "학사력 수정본 금요일까지 교육청에 제출.";
  const nd2 = await app.invoke("create_note", { raw: D2 });
  const planD2 = await app.invoke("suggest_plan", { id: nd2.id });
  const dTask = planD2.items.find((i) => i.kind === "task");
  check("시나리오 D: 후속조치만 할 일로 (업무 연결)", planD2.items.some((i) => i.kind === "duty") && dTask?.date === FRI && dTask?.duty_id, JSON.stringify(planD2.items.map((i) => [i.kind, i.text, i.date])));
  await app.invoke("save_plan", { id: nd2.id, plan: planD2 });
  await app.nav("업무");
  await sleep(900);
  await app.click("교무기획", ".duty-list button");
  await sleep(700);
  await app.screenshot(path.join(shots, "08-duties.png"));

  // 오늘 화면: 기한 지난 → 오늘 → 앞으로 순서
  await app.invoke("create_task", { input: { title: "지난주 설문 결과 입력", due_date: "2026-09-25", due_time: null, date_expr: null, duty_id: null } });
  await app.invoke("create_task", { input: { title: "학급 게시판 정리", due_date: "2026-09-29", due_time: "16:00", date_expr: null, duty_id: null } });
  await app.nav("오늘");
  await sleep(900);
  const order = await app.evaluate(`[...document.querySelectorAll('.today .bucket h3')].map(h => h.innerText.split('\\n')[0])`);
  check("오늘 화면 순서: 기한 지난 → 오늘 → 앞으로", order[0]?.startsWith("기한 지난") && order[1]?.startsWith("오늘 할 일") && order[2]?.startsWith("앞으로 할 일"), order.join(" | "));
  await app.screenshot(path.join(shots, "09-today-with-tasks.png"));

  // 학생 상세
  await app.nav("학생");
  await sleep(800);
  await app.screenshot(path.join(shots, "10-students.png"));
  await app.click("홍길동", ".student-card");
  await sleep(800);
  await app.evaluate(`window.__e2e.click('원문 보기')`);
  await sleep(400);
  await app.screenshot(path.join(shots, "11-student-detail.png"));
  const detail = await app.text(".student-detail");
  check("학생 상세에 작성 시각·원문·영역 표시", detail.includes("출결특기사항") && detail.includes("원문") && detail.includes("보호자 연락"));

  // 검색
  await app.nav("검색");
  await sleep(300);
  await app.type(".search-big", "지각");
  await sleep(1200);
  await app.screenshot(path.join(shots, "12-search.png"));
  const sr = await app.invoke("search", { query: { text: "공감소통" } });
  check("FTS 검색: 공감소통 → 1건", sr.notes.length === 1, String(sr.notes.length));

  // 설정 (모델 카탈로그·기기정보)
  await app.nav("설정");
  await waitFor(() => app.evaluate(`document.querySelectorAll('.model-card').length >= 5`), { label: "모델 카드" });
  await sleep(1500);
  await app.screenshot(path.join(shots, "13-settings-models.png"));
  const cat = await app.invoke("model_catalog");
  check("모델 5단계 + 대안", new Set(cat.entries.filter((e) => !e.model.alternative).map((e) => e.model.tier)).size === 5, cat.entries.map((e) => e.model.tier_label).join(","));
  check("기기 사양 확인·권장 모델", !!cat.device && !!cat.device.recommended_model, cat.device ? `${cat.device.cpu_name} / RAM ${Math.round(cat.device.ram_total_bytes / 2 ** 30)}GB / 권장 ${cat.device.recommended_model}` : "없음");
  const checkedAt = cat.device?.checked_at;
  await app.nav("오늘");
  await sleep(300);
  await app.nav("설정");
  await sleep(1200);
  const cat2 = await app.invoke("model_catalog");
  check("설정을 다시 열어도 기기정보를 다시 읽지 않음", cat2.device?.checked_at === checkedAt, `${checkedAt} → ${cat2.device?.checked_at}`);
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  await fs.writeFile(path.join(shots, "results-noai.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
