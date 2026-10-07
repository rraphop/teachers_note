// 0.2 기능 E2E: 업무 세부업무 통합, 학사력 하루/기간, 시간표(일과 시간·개인·학급·전체·오늘), 학급명렬표·생일, 업데이트 기록
// 실행: node scripts/e2e/run-v02.mjs <exe> <데이터폴더> <스크린샷폴더>
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
const now = new Date();
const todayMD = `${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;

const child = launch(exe, { dataRoot });
let app;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  const info = await app.invoke("app_info");
  check("버전 0.2 이상", /^0\.([2-9]|\d{2,})\./.test(info.version) || /^[1-9]\d*\./.test(info.version), info.version);

  // ── 1. 업무: 업무명 + 세부업무 두 칸 ──
  await app.nav("업무");
  await sleep(500);
  await app.click("담당업무 등록", ".head-actions button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.dialog textarea')`), { label: "업무 등록 창" });
  const dutyDialog = await app.text(".dialog");
  check("업무 등록 창: '분류에 쓸 설명'·'핵심어' 칸 없음, 세부업무 하나", !dutyDialog.includes("분류에 쓸 설명") && !dutyDialog.includes("핵심어 (") && dutyDialog.includes("세부업무"));
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.dialog input'), '교무기획')`);
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.dialog textarea'), '학사일정 관리, 학사력, 수업일수\\n교육청에 학사일정 변경을 보고하고 교직원에게 안내함')`);
  await sleep(200);
  await app.screenshot(path.join(shots, "30-duty-form.png"));
  await app.click("저장", ".dialog-foot button");
  await sleep(600);
  const duties = await app.invoke("list_duties");
  check("세부업무 저장", duties[0]?.detail.includes("수업일수") && duties[0]?.guide === "" && duties[0]?.keywords === "", duties[0]?.detail);
  const n1 = await app.invoke("create_note", { raw: "수업일수 계산해서 학사력에 반영하기" });
  const p1 = await app.invoke("suggest_plan", { id: n1.id });
  check("세부업무의 짧은 낱말로 업무 분류(AI 없이)", p1.items.some((i) => i.kind === "duty" && i.duty_id === duties[0].id), JSON.stringify(p1.items.map((i) => i.kind)));
  const n2 = await app.invoke("create_note", { raw: "교육청 공문 회신 금요일까지" });
  const p2 = await app.invoke("suggest_plan", { id: n2.id });
  check("세부업무 문장 속 흔한 낱말(교육청)로는 업무 분류 안 함", p2.items.every((i) => i.kind !== "duty"), JSON.stringify(p2.items.map((i) => i.kind)));
  await sleep(300);
  await app.screenshot(path.join(shots, "31-duties.png"));

  // ── 2. 학사력: 시각 없음, 기본 하루, 체크하면 기간 ──
  await app.nav("일정표");
  await sleep(500);
  await app.click("학사력", ".seg button");
  await sleep(500);
  await app.click("학사력 추가", ".head-actions button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.dialog')`), { label: "학사력 추가 창" });
  const timeInputs = await app.evaluate(`document.querySelectorAll('.dialog input[type=time]').length`);
  const dateInputs1 = await app.evaluate(`document.querySelectorAll('.dialog input[type=date]').length`);
  check("학사력 추가: 시각 칸 없음, 날짜 하나(하루)", timeInputs === 0 && dateInputs1 === 1, `time=${timeInputs} date=${dateInputs1}`);
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.dialog input[placeholder="예: 2학기 개학식"]'), '2학기 기말고사')`);
  await app.evaluate(`[...document.querySelectorAll('.dialog label.check')].find(l => l.innerText.includes('기간으로 설정')).querySelector('input').click()`);
  await sleep(300);
  const dateInputs2 = await app.evaluate(`document.querySelectorAll('.dialog input[type=date]').length`);
  check("'기간으로 설정' 체크 → 끝 날짜 칸 나타남", dateInputs2 === 2, String(dateInputs2));
  await app.evaluate(`(() => { const d = document.querySelectorAll('.dialog input[type=date]'); window.__e2e.setValue(d[0], '2026-12-14'); })()`);
  await sleep(150);
  await app.evaluate(`(() => { const d = document.querySelectorAll('.dialog input[type=date]'); window.__e2e.setValue(d[1], '2026-12-18'); })()`);
  await sleep(200);
  await app.screenshot(path.join(shots, "32-annual-period.png"));
  await app.click("저장", ".dialog-foot button");
  await sleep(600);
  const yearItems = await app.invoke("calendar", { view: "year", start: "2026-03-01", end: "2027-02-28" });
  const exam = yearItems.find((i) => i.title === "2학기 기말고사");
  check("기간 학사력 저장 (12/14~12/18, 시각 없음)", exam?.start_date === "2026-12-14" && exam?.end_date === "2026-12-18" && !exam?.time, JSON.stringify(exam));
  await app.click("학사력 추가", ".head-actions button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.dialog')`), { label: "학사력 추가 창 2" });
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.dialog input[placeholder="예: 2학기 개학식"]'), '개교기념일')`);
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.dialog input[type=date]'), '2026-10-23')`);
  await sleep(150);
  await app.click("저장", ".dialog-foot button");
  await sleep(600);
  const one = (await app.invoke("calendar", { view: "year", start: "2026-03-01", end: "2027-02-28" })).find((i) => i.title === "개교기념일");
  check("기본은 하루짜리 학사력", one?.start_date === "2026-10-23" && one?.end_date === "2026-10-23", JSON.stringify(one));
  await app.screenshot(path.join(shots, "33-calendar-year-schoolyear.png"));

  // ── 3. 시간표 ──
  await app.nav("시간표");
  await sleep(700);
  await app.click("일과 시간 설정", ".seg.tabs button");
  await sleep(500);
  await app.click("고등학교 (50분)", ".bell button");
  await sleep(400);
  await app.evaluate(`[...document.querySelectorAll('.bell label.check')].find(l => l.innerText.includes('0교시 사용')).querySelector('input').click()`);
  await sleep(400);
  await app.click("방과후 추가", ".bell button");
  await sleep(400);
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.after-school input:not([type])'), '방과후 영어')`);
  await sleep(400);
  await app.screenshot(path.join(shots, "34-bell-settings.png"));
  await app.click("일과 시간 저장", ".bell button");
  await sleep(700);
  const sem = await app.invoke("current_semester");
  const bell = await app.invoke("get_bell", { year: sem.school_year, semester: sem.semester });
  const nos = bell.periods.map((p) => p.no);
  check("일과 시간 저장: 50분·0교시·방과후", bell.saved && bell.config.period_minutes === 50 && nos.includes(0) && bell.periods.some((p) => p.kind === "after" && p.label === "방과후 영어"), bell.periods.map((p) => `${p.label} ${p.start}-${p.end}`).join(" / "));

  await app.click("개인 시간표", ".seg.tabs button");
  await sleep(600);
  const clickCell = (label, col) =>
    app.evaluate(`(() => { const tr = [...document.querySelectorAll('.tt-grid tbody tr')].find(r => r.querySelector('.tt-period b')?.innerText === '${label}'); tr.querySelectorAll('td')[${col}].click(); return true; })()`);
  const typeCell = async (text, key) => {
    await waitFor(() => app.evaluate(`!!document.querySelector('.tt-cell.editing input')`), { label: "칸 입력" });
    await app.evaluate(`window.__e2e.setValue(document.querySelector('.tt-cell.editing input'), ${JSON.stringify(text)})`);
    await app.evaluate(`document.querySelector('.tt-cell.editing input').dispatchEvent(new KeyboardEvent('keydown', { key: '${key}', bubbles: true }))`);
    await sleep(400);
  };
  await clickCell("2교시", 2);
  await typeCell("2-1 국어 (과학실)", "Enter");
  await typeCell("2-3 국어", "Enter");
  await app.evaluate(`document.querySelector('.tt-cell.editing input')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))`);
  await sleep(300);
  await clickCell("1교시", 1);
  await typeCell("1-2 국어", "Tab");
  await typeCell("1-2 국어", "Escape");
  await sleep(400);
  const personal = await app.invoke("get_timetable", { year: sem.school_year, semester: sem.semester, kind: "personal", classLabel: null });
  const c22 = personal.cells.find((c) => c.weekday === 2 && c.period === 2);
  const c23 = personal.cells.find((c) => c.weekday === 2 && c.period === 3);
  check("개인 시간표: 한 줄 입력 → 학급·과목·장소로 저장, Enter 로 아래 칸", c22?.class_label === "2-1" && c22?.subject === "국어" && c22?.room === "과학실" && c23?.class_label === "2-3", JSON.stringify(personal.cells.map((c) => [c.weekday, c.period, c.class_label, c.subject, c.room])));
  check("Tab 으로 오른쪽 칸(월 1교시 → 화 1교시)", personal.cells.some((c) => c.weekday === 1 && c.period === 1), "");
  await app.screenshot(path.join(shots, "35-timetable-personal.png"));

  await app.click("학급 시간표", ".seg.tabs button");
  await sleep(500);
  await app.evaluate(`window.__e2e.setValue(document.querySelector('input[placeholder="새 학급 (예: 2-3)"]'), '2-1')`);
  await app.click("열기", ".tt-classbar button");
  await sleep(600);
  await clickCell("1교시", 1);
  await typeCell("수학 김민정 (2-1교실)", "Enter");
  await typeCell("국어 박선생", "Escape");
  await sleep(300);
  const cls = await app.invoke("get_timetable", { year: sem.school_year, semester: sem.semester, kind: "class", classLabel: "2-1" });
  check("학급 시간표(2-1): 과목·선생님·장소", cls.cells.some((c) => c.subject === "수학" && c.teacher === "김민정" && c.room === "2-1교실"), JSON.stringify(cls.cells.map((c) => [c.subject, c.teacher, c.room])));
  await app.screenshot(path.join(shots, "36-timetable-class.png"));

  await app.click("전체 시간표", ".seg.tabs button");
  await sleep(600);
  await app.evaluate(`window.__e2e.setValue(document.querySelector('input[placeholder="학급 추가 (예: 3-2)"]'), '1-1')`);
  await app.click("학급 줄 추가");
  await sleep(700);
  await app.evaluate(`(() => { const tr = [...document.querySelectorAll('.tt-grid.school tbody tr')].find(r => r.querySelector('.tt-period b')?.innerText === '1-1'); tr.querySelectorAll('td')[1].click(); return true; })()`);
  await typeCell("국어 박선생", "Tab");
  await typeCell("영어 이선생", "Escape");
  await sleep(400);
  const school = await app.invoke("get_timetable", { year: sem.school_year, semester: sem.semester, kind: "school", classLabel: null });
  check("전체 시간표: 학급 줄 추가·입력", school.rows.includes("1-1") && school.cells.some((c) => c.row_label === "1-1" && c.subject === "국어" && c.teacher === "박선생"), JSON.stringify(school.rows));
  await app.screenshot(path.join(shots, "37-timetable-school.png"));

  // ── 4. 학급명렬표 ──
  await app.nav("학생");
  await sleep(500);
  await app.click("학급명렬표", ".seg button");
  await sleep(600);
  const addRow = async (cls, num, name) => {
    const inputs = `document.querySelectorAll('.add-row input')`;
    await app.evaluate(`window.__e2e.setValue(${inputs}[0], '${cls}')`);
    await app.evaluate(`window.__e2e.setValue(${inputs}[1], '${num}')`);
    await app.evaluate(`window.__e2e.setValue(${inputs}[2], '${name}')`);
    await app.evaluate(`${inputs}[2].dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
    await sleep(600);
  };
  await addRow("2-1", "3", "홍길동");
  await addRow("2-1", "7", "성춘향");
  const heads0 = await app.evaluate(`[...document.querySelectorAll('.roster-table th')].map(t => t.innerText)`);
  check("기본 명렬표: 학급·번호·이름만 (선택 항목은 꺼짐)", heads0.join(",") === "학급,번호,이름,기록", heads0.join(","));
  for (const label of ["전화번호", "성별", "생일", "특이사항"]) {
    await app.evaluate(`[...document.querySelectorAll('.roster-cols label')].find(l => l.innerText.includes('${label}')).querySelector('input').click()`);
    await sleep(500);
  }
  const heads1 = await app.evaluate(`[...document.querySelectorAll('.roster-table th')].map(t => t.innerText)`);
  check("체크한 항목만 열로 나타남", ["전화번호", "성별", "생일", "특이사항"].every((h) => heads1.includes(h)) && !heads1.includes("주소"), heads1.join(","));
  const editCell = async (rowName, colName, value) => {
    await app.evaluate(`(() => {
      const heads = [...document.querySelectorAll('.roster-table th')].map(t => t.innerText);
      const ci = heads.indexOf('${colName}');
      const tr = [...document.querySelectorAll('.roster-table tbody tr')].find(r => r.querySelectorAll('td')[2]?.innerText.includes('${rowName}'));
      tr.querySelectorAll('td')[ci].click(); return true; })()`);
    await sleep(250);
    await app.evaluate(`(() => { const el = document.querySelector('.roster-cell.editing input, .roster-cell.editing select'); window.__e2e.setValue(el, ${JSON.stringify(value)}); el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); return true; })()`);
    await sleep(600);
  };
  await editCell("홍길동", "전화번호", "010-1234-5678");
  await editCell("홍길동", "성별", "남");
  await editCell("홍길동", "생일", `2012-${todayMD}`);
  await editCell("홍길동", "특이사항", "알레르기(땅콩) — 급식 확인");
  await editCell("성춘향", "생일", "3월 5일");
  const st = (await app.invoke("list_students", { query: "홍길동" }))[0];
  check("명렬표 칸 바로 고치기 (전화·성별·생일·특이사항)", st.phone === "010-1234-5678" && st.gender === "남" && st.birthday === `2012-${todayMD}` && st.memo.includes("땅콩"), JSON.stringify([st.phone, st.gender, st.birthday, st.memo]));
  await app.evaluate(`[...document.querySelectorAll('.roster-bar label.check')].find(l => l.innerText.includes('생일을 일정표')).querySelector('input').click()`);
  await sleep(600);
  const s = await app.invoke("get_settings");
  check("생일을 일정표에 표시 설정", s.birthdays_on_calendar === true && s.roster_columns.birthday === true, "");
  await app.evaluate(`[...document.querySelectorAll('.roster-bar label.check')].find(l => l.innerText.includes('가리기')).querySelector('input').click()`);
  await sleep(400);
  const masked = await app.text(".roster-table");
  check("연락처 가리기", masked.includes("●●●-●●●●-5678") && !masked.includes("010-1234-5678"), "");
  await app.screenshot(path.join(shots, "38-roster.png"));
  const y = now.getFullYear();
  const lastDay = new Date(y, now.getMonth() + 1, 0).getDate();
  const monthItems = await app.invoke("calendar", { view: "month", start: `${y}-${pad(now.getMonth() + 1)}-01`, end: `${y}-${pad(now.getMonth() + 1)}-${pad(lastDay)}` });
  check("생일이 일정표에 표시", monthItems.some((i) => i.kind === "birthday" && i.title === "2-1 홍길동 생일"), monthItems.filter((i) => i.kind === "birthday").map((i) => i.title).join(","));
  const ov = await app.invoke("today_overview");
  check("오늘 화면: 오늘 생일", ov.birthdays_today.includes("2-1 홍길동"), ov.birthdays_today.join(","));

  // ── 오늘 화면: 오늘 시간표 ──
  await app.nav("오늘");
  await sleep(900);
  const todayText = await app.text(".today-tt");
  const wd = now.getDay();
  if (wd >= 1 && wd <= 5) check("오늘 화면: 오늘 시간표 표시", todayText.includes("오늘 시간표") && (wd !== 2 || todayText.includes("2-1")), todayText.replace(/\n/g, " ").slice(0, 120));
  else check("오늘 화면: 주말에는 수업 없음 안내", todayText.includes("수업이 없는 날"), todayText.slice(0, 80));
  await app.screenshot(path.join(shots, "39-today-timetable.png"));

  // ── 5. 업데이트 기록 ──
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.changelog')`), { label: "업데이트 기록" });
  const log = await app.text(".dialog");
  check("왼쪽 아래 업데이트 기록: v1.0 기능 소개 (시간표·학생 명렬표)", log.includes("v1.0") && log.includes("시간표") && log.includes("학생 명렬표") && !log.includes("v0."), "");
  await app.screenshot(path.join(shots, "40-update-log.png"));
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  await fs.writeFile(path.join(shots, "results-v02.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
