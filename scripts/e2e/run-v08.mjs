// 0.8 E2E: 학교 설정(나이스 학교 찾기·담임반·인증키), 급식(오늘 화면), 나이스 시간표 → 학교·학급·개인 시간표(내 과목),
//          학사력(종류 색·방학 묶기·토요휴업일 숨김·학년 태그·규칙 바꾸기·지운 일정 유지), 시간표 색칠하기
// 실제 나이스 대신 이 PC 안의 시험 서버(127.0.0.1)를 쓴다 (TN_NEIS_BASE). 학교·자료는 모두 가짜.
// 실행: node scripts/e2e/run-v08.mjs <exe> <데이터폴더> <스크린샷폴더>
import fs from "node:fs/promises";
import http from "node:http";
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
const iso = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const ymd = (d) => iso(d).replace(/-/g, "");
const fromYmd = (s) => new Date(Number(s.slice(0, 4)), Number(s.slice(4, 6)) - 1, Number(s.slice(6, 8)));
const addDays = (d, n) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
const now = new Date();
const today = iso(now);
// 급식 없는 날: 오늘 다음의 첫 평일
let skip = addDays(now, 1);
while (skip.getDay() === 0 || skip.getDay() === 6) skip = addDays(skip, 1);
const skipDay = iso(skip);
let nextMeal = addDays(skip, 1);
while (nextMeal.getDay() === 0 || nextMeal.getDay() === 6) nextMeal = addDays(nextMeal, 1);

// ───── 시험용 나이스 서버 ─────
const PORT = 18765;
const KEY = "TESTKEY123";
const requests = [];
const okBody = (service, rows, total) => JSON.stringify({ [service]: [{ head: [{ list_total_count: total }, { RESULT: { CODE: "INFO-000", MESSAGE: "정상 처리되었습니다." } }] }, { row: rows }] });
const none = JSON.stringify({ RESULT: { CODE: "INFO-200", MESSAGE: "해당하는 데이터가 없습니다." } });
function page(service, all, q) {
  const key = q.get("KEY");
  const size = key ? Number(q.get("pSize") || 100) : 5;
  const idx = key ? Number(q.get("pIndex") || 1) : 1;
  const rows = all.slice((idx - 1) * size, idx * size);
  return rows.length ? okBody(service, rows, all.length) : none;
}
const SCHOOL = { ATPT_OFCDC_SC_CODE: "Q99", ATPT_OFCDC_SC_NM: "OO교육청", SD_SCHUL_CODE: "9990001", SCHUL_NM: "OO중학교", SCHUL_KND_SC_NM: "중학교", LCTN_SC_NM: "OO도", ORG_RDNMA: "OO시 OO로 1" };
const OTHER = { ...SCHOOL, SD_SCHUL_CODE: "9990002", SCHUL_NM: "OO고등학교", SCHUL_KND_SC_NM: "고등학교" };
const SUBJ = ["국어", "영어", "수학", "과학", "사회", "체육", "음악", "미술", "도덕", "기술·가정"];
function subjectOf(cls, wd, p) {
  if ((cls === "1-1" && wd === 1 && p === 1) || (cls === "2-1" && wd === 2 && p === 3) || (cls === "3-1" && wd === 3 && p === 5)) return "정보";
  return SUBJ[(wd * 7 + p + Number(cls[0]) * 3) % SUBJ.length];
}
function daysBetween(a, b) {
  const out = [];
  for (let d = fromYmd(a); d <= fromYmd(b); d = addDays(d, 1)) out.push(d);
  return out;
}
const scheduleRows = [
  ["20261003", "개천절", "공휴일"],
  ["20261009", "한글날", "공휴일"],
  ["20261010", "토요휴업일", "휴업일"],
  ["20261013", "중간고사②", "해당없음"],
  ["20261014", "중간고사②", "해당없음"],
  ["20261021", "체육대회", "해당없음"],
  ["20261102", "재량휴업일", "휴업일"],
  ...["20261224", "20261228", "20261229", "20261230", "20261231"].map((d) => [d, "겨울방학", "휴업일"]),
].map(([d, n, k]) => ({ ...SCHOOL, AA_YMD: d, EVENT_NM: n, SBTR_DD_SC_NM: k, ONE_GRADE_EVENT_YN: "Y", TW_GRADE_EVENT_YN: "Y", THREE_GRADE_EVENT_YN: "Y" }));

const server = http.createServer((req, res) => {
  const u = new URL(req.url, `http://127.0.0.1:${PORT}`);
  const service = u.pathname.split("/").pop();
  const q = u.searchParams;
  requests.push({ service, params: [...q.keys()], hasKey: q.has("KEY") });
  let body = none;
  if (service === "schoolInfo") body = page(service, [SCHOOL, OTHER].filter((s) => s.SCHUL_NM.includes(q.get("SCHUL_NM") || "")), q);
  else if (service === "classInfo") body = page(service, ["1", "2", "3"].map((g) => ({ ...SCHOOL, AY: q.get("AY"), GRADE: g, CLASS_NM: "1" })), q);
  else if (service === "misTimetable") {
    const rows = [];
    for (const d of daysBetween(q.get("TI_FROM_YMD"), q.get("TI_TO_YMD"))) {
      const wd = d.getDay();
      if (wd === 0 || wd === 6) continue;
      for (const cls of ["1-1", "2-1", "3-1"])
        for (let p = 1; p <= 6; p++) rows.push({ ...SCHOOL, AY: "2026", SEM: "2", ALL_TI_YMD: ymd(d), GRADE: cls[0], CLASS_NM: "1", PERIO: String(p), ITRT_CNTNT: subjectOf(cls, wd, p) });
    }
    body = page(service, rows, q);
  } else if (service === "mealServiceDietInfo") {
    const from = q.get("MLSV_YMD") || q.get("MLSV_FROM_YMD");
    const to = q.get("MLSV_YMD") || q.get("MLSV_TO_YMD");
    const rows = daysBetween(from, to)
      .filter((d) => d.getDay() !== 0 && d.getDay() !== 6 && iso(d) !== skipDay)
      .map((d) => ({
        ...SCHOOL,
        MLSV_YMD: ymd(d),
        MMEAL_SC_NM: "중식",
        DDISH_NM: `현미밥 <br/>미역국 (5.6.9)<br/>제육볶음 (5.6.10.13)<br/>배추김치 (9.13)<br/>${d.getDate()}일 후식`,
        CAL_INFO: "780.2 Kcal",
        NTR_INFO: "탄수화물(g) : 110.2<br/>단백질(g) : 30.1",
        ORPLC_INFO: "쌀 : 국내산<br/>돼지고기 : 국내산",
        MLSV_FGR: "300",
      }));
    body = page(service, rows, q);
  } else if (service === "SchoolSchedule") body = page(service, scheduleRows, q);
  res.writeHead(200, { "Content-Type": "application/json; charset=utf-8" });
  res.end(body);
});
await new Promise((r) => server.listen(PORT, "127.0.0.1", r));

const child = launch(exe, { dataRoot, extraEnv: { TN_NEIS_BASE: `http://127.0.0.1:${PORT}/hub` } });
let app;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  const info = await app.invoke("app_info");
  check("버전 0.8 이상", /^0\.([8-9]|\d{2,})\./.test(info.version) || /^[1-9]\d*\./.test(info.version), info.version);

  // ── 1. 오늘 화면: 학교를 정하기 전에는 급식 안내 ──
  await sleep(600);
  const before = await app.text("#today-meals");
  check("학교를 정하기 전: 급식 칸에 안내 + 설정 이동 버튼", before.includes("학교를 정하면") && before.includes("설정에서 학교 정하기"), before.replace(/\n/g, " ").slice(0, 80));

  // ── 2. 설정 → 학교 찾기·고르기 ──
  await app.click("설정에서 학교 정하기", "#today-meals button");
  await waitFor(() => app.evaluate(`!!document.querySelector('#set-school')`), { label: "학교 설정" });
  await app.type("#set-school .school-search input", "OO");
  await app.click("찾기", "#set-school button");
  await waitFor(() => app.evaluate(`document.querySelectorAll('#set-school .school-results li').length === 2`), { label: "학교 찾기 결과" });
  const found = await app.text("#set-school .school-results");
  check("학교 찾기: 2곳, 지역·교육청 표시", found.includes("OO중학교") && found.includes("OO도") && found.includes("OO교육청"), found.replace(/\n/g, " ").slice(0, 100));
  await app.screenshot(path.join(shots, "70-school-search.png"));
  await app.evaluate(`(() => { const li = [...document.querySelectorAll('#set-school .school-results li')].find(l => l.innerText.includes('OO중학교')); li.querySelector('button').click(); return true; })()`);
  await waitFor(async () => (await app.invoke("get_settings")).school?.code === "9990001", { label: "학교 저장" });
  const st1 = await waitFor(async () => {
    const s = await app.invoke("neis_status");
    return s.classes.length === 3 && s.timetable ? s : null;
  }, { label: "학급·시간표 상태" });
  check("학교 고르면 학급 목록 받음 (1-1·2-1·3-1)", st1.classes.join(",") === "1-1,2-1,3-1", st1.classes.join(","));
  check("인증키 없이: 시간표·학사일정은 ‘인증키 필요’로 받지 않음", st1.timetable?.status === "need_key" && st1.schedule?.status === "need_key", `${st1.timetable?.status}/${st1.schedule?.status}`);
  const noKeyTt = requests.filter((r) => r.service === "misTimetable" || r.service === "SchoolSchedule");
  check("인증키 없이 시간표·학사일정을 쪼개 받지 않음 (요청 0번)", noKeyTt.length === 0, String(noKeyTt.length));
  // 담임반 (학급 목록이 화면에 채워진 뒤)
  await waitFor(() => app.evaluate(`!!document.querySelector('#set-school select option[value="1-1"]')`), { label: "담임반 목록" });
  await app.evaluate(`(() => { const sel = [...document.querySelectorAll('#set-school select')][0]; window.__e2e.setValue(sel, '1-1'); return true; })()`);
  await waitFor(async () => (await app.invoke("get_settings")).homeroom === "1-1", { label: "담임반 저장" });
  check("담임반 1-1 저장", true);
  await app.screenshot(path.join(shots, "71-school-settings.png"));

  // ── 3. 오늘 화면 급식 ──
  await app.nav("오늘");
  const meals = await waitFor(async () => {
    const t = await app.text("#today-meals");
    return t.includes("미역국") || t.includes("급식이 없습니다") ? t : null;
  }, { label: "급식" });
  const weekend = now.getDay() === 0 || now.getDay() === 6;
  check(
    weekend ? "주말: 오늘은 급식이 없습니다 + 다음 급식" : "오늘 급식: 메뉴 한 줄에 하나 + 열량",
    weekend ? meals.includes("급식이 없습니다") : meals.includes("현미밥") && meals.includes("미역국") && meals.includes("780.2 Kcal"),
    meals.replace(/\n/g, " / ").slice(0, 120),
  );
  check("알레르기 번호는 기본으로 숨김", !(await app.evaluate(`!!document.querySelector('#today-meals .allergens')`)), "");
  await app.click("알레르기 번호 보기", "#today-meals button");
  await sleep(200);
  const al = await app.evaluate(`[...document.querySelectorAll('#today-meals .allergens')].map(e => e.innerText + '|' + e.title).join(' ')`);
  check("누르면 알레르기 번호 표시 (이름은 풍선 도움말)", al.includes("5.6.9") && al.includes("대두"), al.slice(0, 80));
  await app.screenshot(path.join(shots, "72-today-meals.png"));
  // 급식 없는 날 → 다음 급식일 미리 보기
  for (let i = 0; i < 10; i++) {
    const shown = await app.text("#today-meals .meal-date");
    if (shown.includes(`${skip.getMonth() + 1}월 ${skip.getDate()}일`)) break;
    await app.evaluate(`document.querySelector('#today-meals button[aria-label="하루 뒤"]').click()`);
    await sleep(500);
  }
  const noMeal = await waitFor(async () => {
    const t = await app.text("#today-meals");
    return t.includes("이날은 급식이 없습니다") ? t : null;
  }, { label: "급식 없는 날", timeout: 15000 }).catch(() => "");
  check("급식 없는 날: 안내 + 다음 급식일 미리 보기", noMeal.includes("다음 급식") && noMeal.includes(`${nextMeal.getMonth() + 1}월 ${nextMeal.getDate()}일`), noMeal.replace(/\n/g, " / ").slice(0, 120));
  await app.screenshot(path.join(shots, "73-meals-next.png"));

  // ── 4. 인증키 넣기 → 시간표·학사일정 받기 ──
  await app.nav("설정");
  await waitFor(() => app.evaluate(`!!document.querySelector('#set-school .neis-key input')`), { label: "인증키 칸" });
  await app.type("#set-school .neis-key input", KEY);
  await app.click("저장", "#set-school .neis-key button");
  const st2 = await waitFor(async () => {
    const s = await app.invoke("neis_status");
    return s.timetable?.status === "ok" && s.schedule?.status === "ok" ? s : null;
  }, { label: "시간표·학사일정 받기", timeout: 30000 });
  check("인증키를 넣으면 시간표·학사일정 받음", !!st2, "");
  check("학교 시간표 과목 목록", st2.subjects.includes("정보") && st2.subjects.includes("국어"), st2.subjects.join(","));
  const allowed = new Set(["Type", "pIndex", "pSize", "KEY", "ATPT_OFCDC_SC_CODE", "SD_SCHUL_CODE", "SCHUL_NM", "AY", "TI_FROM_YMD", "TI_TO_YMD", "MLSV_FROM_YMD", "MLSV_TO_YMD", "MLSV_YMD", "AA_FROM_YMD", "AA_TO_YMD"]);
  const extra = requests.flatMap((r) => r.params.filter((p) => !allowed.has(p)));
  check("나이스에 보낸 값: 학교 코드·날짜·학교 이름·인증키뿐", extra.length === 0 && requests.length > 0, extra.length ? extra.join(",") : `요청 ${requests.length}번`);
  const school = await app.invoke("get_timetable", { year: 2026, semester: 2, kind: "school", classLabel: null });
  check("학교 시간표: 3학급 × 5일 × 6교시", school.cells.length === 90 && school.cells.every((c) => c.source === "neis"), String(school.cells.length));
  const cls = await app.invoke("get_timetable", { year: 2026, semester: 2, kind: "class", classLabel: "2-1" });
  check("학급 시간표도 만듦 (2-1)", cls.cells.length === 30, String(cls.cells.length));

  // ── 5. 내 과목 → 개인 시간표 ──
  await app.evaluate(`(() => { const l = [...document.querySelectorAll('#set-school .check-grid label')].find(x => x.innerText.trim() === '정보'); l.querySelector('input').click(); return true; })()`);
  const personal = () => app.invoke("get_timetable", { year: 2026, semester: 2, kind: "personal", classLabel: null });
  const p1 = await waitFor(async () => {
    const v = await personal();
    return v.cells.length === 3 ? v : null;
  }, { label: "개인 시간표" });
  check("내 과목 ‘정보’ → 개인 시간표 3칸 (1-1 월1·2-1 화3·3-1 수5)", p1.cells.map((c) => `${c.class_label}:${c.weekday}-${c.period}`).sort().join(" ") === "1-1:1-1 2-1:2-3 3-1:3-5", p1.cells.map((c) => `${c.class_label}:${c.weekday}-${c.period}`).join(" "));
  await sleep(400);
  await app.evaluate(`(() => { const l = [...document.querySelectorAll('#set-school .check-grid label')].find(x => x.innerText.trim() === '3-1'); l.querySelector('input').click(); return true; })()`);
  const p2 = await waitFor(async () => {
    const v = await personal();
    return v.cells.length === 2 ? v : null;
  }, { label: "학급 빼기" }).catch(() => null);
  check("학급 빼기(3-1) → 개인 시간표 2칸", !!p2, "");
  await app.screenshot(path.join(shots, "74-my-subjects.png"));

  // ── 6. 시간표 색칠하기 ──
  await app.nav("시간표");
  await sleep(800);
  await app.click("개인 시간표", ".seg.tabs button");
  await sleep(500);
  const bar = await app.text(".neis-bar");
  check("시간표 화면: 나이스 받은 때 표시", bar.includes("OO중학교") && bar.includes("받음"), bar.replace(/\n/g, " "));
  await app.click("색칠하기", ".page-head button");
  await sleep(200);
  const cellStyle = (i, sel = ".tt-grid td.tt-cell.filled") =>
    app.evaluate(`(() => { const td = document.querySelectorAll('${sel}')[${i}]; return { bg: getComputedStyle(td).backgroundColor, fg: getComputedStyle(td).color, text: td.innerText.replace(/\\s+/g, ' ') }; })()`);
  await app.evaluate(`document.querySelectorAll('.tt-grid td.tt-cell.filled')[0].click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.swatches')`), { label: "색 고르기" });
  const scopes = await app.evaluate(`[...document.querySelectorAll('.dialog .color-scopes button')].map(b => (b.classList.contains('on') ? '*' : '') + b.innerText.trim())`);
  check("색 정하기 순서: 이 칸만 → 교과별 → 학급별 (처음엔 교과별)", JSON.stringify(scopes) === JSON.stringify(["이 칸만", "*교과별 (정보)", "학급별 (1-1)"]), JSON.stringify(scopes));
  await app.click("파랑", ".swatches button");
  await sleep(500);
  const c0 = await cellStyle(0);
  const c1 = await cellStyle(1);
  check("교과별 ‘파랑’ → 같은 과목 칸(1-1·2-1 정보) 모두 파랑 + 흰 글자", c0.bg === "rgb(74, 127, 217)" && c1.bg === "rgb(74, 127, 217)" && c0.fg === "rgb(255, 255, 255)", `${c0.text}:${c0.bg} / ${c1.text}:${c1.bg}`);
  await app.evaluate(`document.querySelectorAll('.tt-grid td.tt-cell.filled')[0].click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.swatches')`), { label: "색 고르기 2" });
  await app.click("이 칸만", ".dialog .color-scopes button");
  await app.click("분홍", ".swatches button");
  await sleep(500);
  const d0 = await cellStyle(0);
  const d1 = await cellStyle(1);
  check("‘이 칸만’ 분홍 → 그 칸만 바뀌고 다른 정보 칸은 파랑 그대로", d0.bg === "rgb(251, 180, 215)" && d1.bg === "rgb(74, 127, 217)", `${d0.bg} / ${d1.bg}`);
  const emptyBg = await app.evaluate(`getComputedStyle(document.querySelector('.tt-grid td.tt-cell:not(.filled)')).backgroundColor`);
  check("공강 칸은 회색", emptyBg === "rgb(238, 240, 238)", emptyBg);
  await app.screenshot(path.join(shots, "75-timetable-colors.png"));
  // 전체 시간표: 교과별 색이 모든 학급의 그 과목 칸에
  await app.click("전체 시간표", ".seg.tabs button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.tt-grid.school td.tt-cell.filled')`), { label: "전체 시간표" });
  await app.evaluate(`[...document.querySelectorAll('.tt-grid.school td.tt-cell.filled')].find(td => td.innerText.trim() === '국어').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.swatches')`), { label: "색 고르기 3" });
  await app.click("초록", ".swatches button");
  await sleep(500);
  const korean = await app.evaluate(`[...document.querySelectorAll('.tt-grid.school td.tt-cell.filled')].filter(td => td.innerText.trim() === '국어').map(td => getComputedStyle(td).backgroundColor)`);
  check("전체 시간표 교과별 ‘초록’ → 모든 학급의 국어 칸", korean.length >= 3 && korean.every((b) => b === "rgb(129, 201, 149)"), `${korean.length}칸 ${[...new Set(korean)].join(",")}`);
  await app.screenshot(path.join(shots, "75b-timetable-subject-colors.png"));
  await app.click("색칠 끝내기", ".page-head button");
  await app.invoke("neis_sync", { what: "timetable" });
  const colorsAfter = await app.invoke("timetable_colors");
  check("시간표를 다시 받아도 교과·칸 색 그대로", colorsAfter.subjects["정보"] === "blue" && colorsAfter.subjects["국어"] === "green" && Object.values(colorsAfter.cells).includes("pink"), JSON.stringify(colorsAfter.subjects));
  // ── 7. 학사력 ──
  await app.nav("일정표");
  await sleep(600);
  const tabs = await app.text(".page-head .seg");
  check("일정표 탭: 주간·월간·학사력", tabs.includes("학사력") && !tabs.includes("연간"), tabs.replace(/\n/g, " "));
  const y = await app.invoke("calendar", { view: "year", start: "2026-03-01", end: "2027-02-28" });
  const by = (t) => y.filter((i) => i.title === t);
  check("한글날 = 휴일", by("한글날")[0]?.day_kind === "holiday", JSON.stringify(by("한글날")[0]?.day_kind));
  check("토요휴업일은 숨김(날짜 색만)", by("토요휴업일")[0]?.hidden === true && by("토요휴업일")[0]?.day_kind === "closure", JSON.stringify(by("토요휴업일")[0] ?? null).slice(0, 80));
  check("겨울방학은 기간 하나 (12/24~12/31)", by("겨울방학").length === 1 && by("겨울방학")[0].start_date === "2026-12-24" && by("겨울방학")[0].end_date === "2026-12-31" && by("겨울방학")[0].day_kind === "vacation", JSON.stringify(by("겨울방학").map((i) => [i.start_date, i.end_date])));
  check("중간고사② → 이름 ‘중간고사’ + 2학년 태그, 이틀 묶음", by("중간고사")[0]?.grades === "2" && by("중간고사")[0]?.end_date === "2026-10-14", JSON.stringify(by("중간고사")[0] ?? null).slice(0, 120));
  check("체육대회 = 업무", by("체육대회")[0]?.day_kind === "work", "");
  await app.click("학사력", ".page-head .seg button");
  await app.evaluate(`document.querySelector('input[type="date"]') && window.__e2e.setValue(document.querySelector('.cal-nav input[type="date"]'), '2026-10-01')`);
  await sleep(800);
  const dayColor = await app.evaluate(`(() => { const m = [...document.querySelectorAll('.mini-month')].find(x => x.querySelector('.mini-title').innerText.trim().endsWith('10월')); const b = [...m.querySelectorAll('.mini-day')].find(x => x.innerText.trim() === '9'); const s = [...m.querySelectorAll('.mini-day')].find(x => x.innerText.trim() === '10'); return { hangul: getComputedStyle(b).color, sat: getComputedStyle(s).color }; })()`);
  check("학사력 보기: 한글날·토요휴업일 날짜가 빨강", dayColor.hangul === "rgb(217, 48, 37)" && dayColor.sat === "rgb(217, 48, 37)", JSON.stringify(dayColor));
  const legend = await app.text(".cal-nav .legend");
  check("학사력 범례: 휴일·휴업일·방학·업무", ["휴일", "휴업일", "방학", "업무"].every((k) => legend.includes(k)), legend.replace(/\n/g, " "));
  await app.screenshot(path.join(shots, "76-school-calendar.png"));

  // 교사가 추가한 학사력은 다시 받아도 그대로, 지운 나이스 일정은 되살리지 않음
  const mine = await app.invoke("create_event", { input: { layer: "annual", title: "학부모 상담 주간", start_date: "2026-10-19", end_date: "2026-10-23" } });
  await app.invoke("delete_event", { id: by("체육대회")[0].id });
  await app.invoke("neis_sync", { what: "schedule" });
  const y2 = await app.invoke("calendar", { view: "year", start: "2026-03-01", end: "2027-02-28" });
  check("다시 받아도 직접 추가한 학사력은 그대로", y2.some((i) => i.id === mine && i.title === "학부모 상담 주간"), "");
  check("지운 나이스 일정(체육대회)은 되살리지 않음", !y2.some((i) => i.title === "체육대회"), y2.filter((i) => i.title === "체육대회").length + "건");

  // 종류 규칙 바꾸기 → 이미 있는 학사력에도 적용
  await app.nav("설정");
  await waitFor(() => app.evaluate(`!!document.querySelector('#set-school textarea')`), { label: "규칙 칸" });
  await app.evaluate(`(() => { const ta = [...document.querySelectorAll('#set-school textarea')][1]; ta.focus(); window.__e2e.setValue(ta, ta.value + ', 학부모 상담'); ta.blur(); return true; })()`);
  const y3 = await waitFor(async () => {
    const r = await app.invoke("calendar", { view: "year", start: "2026-03-01", end: "2027-02-28" });
    return r.find((i) => i.title === "학부모 상담 주간")?.day_kind === "closure" ? r : null;
  }, { label: "규칙 적용", timeout: 10000 }).catch(() => null);
  check("휴업일 규칙에 ‘학부모 상담’ 추가 → 이미 있는 학사력도 휴업일로", !!y3, "");

  // ── 업데이트 기록 ──
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.changelog')`), { label: "업데이트 기록" });
  const log = await app.text(".dialog");
  check("업데이트 기록: v0.8", log.includes("v0.8") && log.includes("급식") && log.includes("학사력") && log.includes("색칠"), "");
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  server.close();
  await fs.writeFile(path.join(shots, "results-v08.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
