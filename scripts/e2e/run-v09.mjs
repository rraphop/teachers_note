// 0.9 E2E: 오늘 업무 요약(학사력·일정·수업·할 일, 시간순, 누르면 이동), 학생부 자료(영역 7개, 교과 과목별·자유학기 활동별,
//          참고자료 옮기기·빼기, 참고자료 모아 붙이기 → 최종안, 학급별 상태, 글자 수·바이트 한도 경고, 최종안 엑셀)
// 실행: node scripts/e2e/run-v09.mjs <exe> <데이터폴더> <스크린샷폴더>
import { execFileSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
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
const now = new Date();
const today = iso(now);
const yesterday = iso(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1));
const wd = now.getDay(); // 0=일
const schoolDay = wd >= 1 && wd <= 5;

const child = launch(exe, { dataRoot });
let app;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  const info = await app.invoke("app_info");
  check("버전 0.9 이상", /^0\.(9|\d{2,})\./.test(info.version) || /^1\./.test(info.version), info.version);

  // ── 준비: 학생·내 과목·시간표·일정·할 일 ──
  const a = await app.invoke("create_student", { input: { name: "심청", class_label: "1-1", number: 1 } });
  await app.invoke("create_student", { input: { name: "전우치", class_label: "1-1", number: 2 } });
  const s0 = await app.invoke("get_settings");
  await app.invoke("update_settings", { value: { ...s0, my_subjects: ["사회", "역사"] } });
  const sem = await app.invoke("current_semester");
  if (schoolDay) {
    await app.invoke("set_timetable_cell", { year: sem.school_year, semester: sem.semester, kind: "personal", classLabel: null, cell: { row_label: "", weekday: wd, period: 1, subject: "사회", teacher: "", class_label: "1-1", room: "", memo: "" } });
  }
  await app.invoke("create_event", { input: { layer: "annual", title: "2학기 중간고사", start_date: today } });
  await app.invoke("create_event", { input: { layer: "weekly", title: "교과협의회", start_date: today, time: "15:30", memo: "본관 3층 회의실" } });
  await app.invoke("create_task", { input: { title: "수행평가 채점", due_date: today } });
  await app.invoke("create_task", { input: { title: "결석계 받기", due_date: yesterday } });

  // ── 1. 오늘 업무 요약 ──
  await app.nav("일정표");
  await sleep(300);
  await app.nav("오늘");
  const brief = await waitFor(async () => {
    const t = await app.text("#today-briefing");
    return t.includes("교과협의회") ? t : null;
  }, { label: "업무 요약" });
  const lines = await app.evaluate(`[...document.querySelectorAll('#today-briefing .brief-lines li')].map(li => ({ text: li.innerText.replace(/\\s+/g, ' ').trim(), top: li.classList.contains('top') }))`);
  check("‘오늘 일정’ 칸이 ‘오늘 업무 요약’으로", brief.includes("오늘 업무 요약") && !brief.includes("오늘 일정이 없습니다"), "");
  check("시험일은 맨 위 (빨간 줄)", lines[0]?.text.includes("오늘은 2학기 중간고사") && lines[0]?.top, JSON.stringify(lines[0]));
  check("일정은 시간·장소와 함께", lines.some((l) => l.text.includes("15:30") && l.text.includes("교과협의회") && l.text.includes("본관 3층")), lines.map((l) => l.text).join(" | "));
  check("오늘 마감 할 일과 기한 지난 할 일", lines.some((l) => l.text.includes("수행평가 채점")) && lines.some((l) => l.text.includes("기한 지난 할 일 1건") && l.text.includes("결석계")), "");
  if (schoolDay) check("오늘 수업 줄 (1교시 1-1 사회, 공강)", lines.some((l) => l.text.includes("1교시 1-1 사회") && l.text.includes("공강")), "");
  check("3~5줄", lines.length >= 3 && lines.length <= 5, String(lines.length));
  const orderOk = (() => {
    const i = lines.findIndex((l) => l.text.includes("교과협의회"));
    const j = lines.findIndex((l) => l.text.includes("수행평가"));
    return i > 0 && j > i;
  })();
  check("시간순 (시간 있는 일정 → 할 일)", orderOk, "");
  const refreshDisabled = await app.evaluate(`document.querySelector('#today-briefing button[aria-label="새로고침"]').disabled`);
  check("AI 모델이 없으면 [새로고침](AI 요약) 버튼은 꺼짐", refreshDisabled === true, "");
  await app.evaluate(`document.getElementById('today-briefing').scrollIntoView({ block: 'start' }); true`);
  await sleep(200);
  await app.screenshot(path.join(shots, "80-today-briefing.png"));
  await app.evaluate(`[...document.querySelectorAll('#today-briefing .brief-line')].find(b => b.innerText.includes('교과협의회')).click()`);
  // 편집 창은 열린 뒤 일정을 불러와 칸을 채운다 — 제목이 채워질 때까지 기다린다
  const opened = await waitFor(
    () => app.evaluate(`(document.querySelector('.dialog')?.innerText || '').includes('교과협의회') || [...document.querySelectorAll('.dialog input')].some(i => i.value === '교과협의회')`),
    { label: "일정 편집 창", timeout: 10000 },
  ).catch(() => false);
  check("줄을 누르면 그 일정 편집 창", opened, "");
  await app.evaluate(`document.querySelector('.dialog [aria-label="닫기"]').click()`);
  await sleep(300);

  // ── 2. 기록 → 학생부 참고자료 ──
  const write = async (raw) => {
    const n = await app.invoke("create_note", { raw });
    const plan = await app.invoke("suggest_plan", { id: n.id });
    await app.invoke("save_plan", { id: n.id, plan });
    return plan;
  };
  await write("1-1 심청 사회 시간 모둠 토의에서 근거를 들어 발표함");
  await write("1-1 심청 주제선택 시간에 지역 조사 결과를 발표함");
  await write("1-1 심청 쉬는 시간에 친구를 도와 교실 청소를 함");
  const rec = await app.invoke("student_record", { studentId: a });
  const tab = (area, subject = "") => rec.find((t) => t.area === area && t.subject === subject);
  check("교과 → 과목 ‘사회’ 칸으로", tab("subject", "사회")?.refs.length === 1 && tab("subject", "역사")?.refs.length === 0, rec.filter((t) => t.area === "subject").map((t) => `${t.subject}:${t.refs.length}`).join(" "));
  check("자유학기 → ‘주제선택활동’ 칸으로", tab("free_semester", "주제선택활동")?.refs.length === 1, rec.filter((t) => t.area === "free_semester").map((t) => `${t.subject}:${t.refs.length}`).join(" "));
  check("행특 칸", tab("behavior")?.refs.length === 1, "");

  // ── 3. 학생부 자료 화면 ──
  await app.nav("학생부 자료");
  await waitFor(() => app.evaluate(`!!document.querySelector('.area-tabs')`), { label: "학생부 화면" });
  const tabsText = await app.text(".area-tabs");
  check("영역 탭 7개", ["창체(자율·자치)", "교과", "행특", "동아리", "진로", "자유학기", "일상생활"].every((k) => tabsText.includes(k)), tabsText.replace(/\n/g, " "));
  await app.evaluate(`[...document.querySelectorAll('.student-pick button')].find(b => b.innerText.includes('심청')).click()`);
  await app.click("교과", ".area-tabs button");
  await waitFor(() => app.evaluate(`(document.querySelector('.ref-list')?.innerText || '').includes('모둠 토의')`), { label: "교과 참고자료" });
  const subs = await app.text(".sub-tabs");
  check("교과 하위 탭: 내 과목(사회·역사)", subs.includes("사회") && subs.includes("역사"), subs.replace(/\n/g, " "));
  // 학급별 상태는 따로 불러오므로 표시가 바뀔 때까지 기다린다
  const markOf = () => app.evaluate(`[...document.querySelectorAll('.student-pick li button')].find(b => b.innerText.includes('심청'))?.querySelector('.rec-mark')?.innerText ?? ''`);
  const mark0 = (await waitFor(async () => ((await markOf()) === "○" ? "○" : null), { label: "학생 상태 표시", timeout: 5000 }).catch(() => null)) ?? (await markOf());
  check("학생 목록: 참고자료만 ○", mark0 === "○", mark0);
  const ai = await app.evaluate(`(() => { const b = [...document.querySelectorAll('.final-box button')].find(x => x.innerText.includes('AI 초안')); return { disabled: b.disabled, title: b.title }; })()`);
  check("AI 모델이 없으면 [AI 초안 만들기] 꺼짐", ai.disabled && ai.title.includes("AI 모델"), ai.title);
  await app.screenshot(path.join(shots, "81-records-subject.png"));
  await app.click("참고자료 모아 붙이기", ".final-box button");
  await waitFor(() => app.evaluate(`document.querySelector('.final-box textarea').value.includes('모둠 토의')`), { label: "모아 붙이기" });
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.final-box textarea'), '모둠 토의에서 근거를 들어 자신의 생각을 발표함.')`);
  await sleep(400);
  const counter = await app.text(".final-box .counter");
  check("글자 수·바이트와 한도 (500자 / 1500바이트)", counter.includes("/ 500자") && counter.includes("/ 1500바이트"), counter);
  await app.click("최종안으로 저장", ".final-box button");
  await waitFor(async () => (await app.invoke("record_status", { area: "subject", subject: "사회" })).find((r) => r.student_id === a)?.status === "final", { label: "최종안 저장" });
  await sleep(500);
  const mark1 = await app.evaluate(`[...document.querySelectorAll('.student-pick li button')].find(b => b.innerText.includes('심청')).querySelector('.rec-mark').innerText`);
  const cls = await app.text(".pick-group .pick-class");
  check("최종안 → 목록 ● + 학급 ‘최종안 1/2’", mark1 === "●" && cls.includes("최종안 1/2"), `${mark1} ${cls}`);
  await app.screenshot(path.join(shots, "82-records-final.png"));
  // 한도 넘기기 (저장하지 않음)
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.final-box textarea'), '가'.repeat(501))`);
  const over = await waitFor(() => app.evaluate(`document.querySelector('.final-box .counter.over') ? document.querySelector('.final-box').innerText : null`), { label: "한도 경고" }).catch(() => "");
  check("한도(500자)를 넘으면 경고", over.includes("나이스 입력 한도"), "");
  await app.screenshot(path.join(shots, "83-records-over.png"));
  // 참고자료 빼기
  await app.click("빼기", ".ref-list button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.ref-row.excluded')`), { label: "빼기" });
  check("참고자료 빼기 → 줄 긋기 표시", true);
  // 자유학기 → 진로로 옮기기
  await app.click("자유학기", ".area-tabs button");
  await app.click("주제선택활동", ".sub-tabs button");
  await waitFor(() => app.evaluate(`(document.querySelector('.ref-list')?.innerText || '').includes('지역 조사')`), { label: "자유학기 참고자료" });
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.ref-row select'), 'career')`);
  const moved = await waitFor(async () => {
    const r = await app.invoke("student_record", { studentId: a });
    return r.find((t) => t.area === "career")?.refs.length === 1 ? r : null;
  }, { label: "영역 옮기기" }).catch(() => null);
  check("참고자료를 다른 영역(진로)으로 옮기기", !!moved && moved.find((t) => t.area === "free_semester" && t.subject === "주제선택활동")?.refs.length === 0, "");

  // ── 4. 최종안 엑셀 ──
  const out = path.join(os.tmpdir(), `tn-finals-${Date.now()}.xlsx`);
  const ex = await app.invoke("export_record_finals", { classId: null, path: out });
  const st = await fs.stat(out).catch(() => null);
  check("학급 최종안 엑셀 저장 (1칸·1명)", ex.rows === 1 && ex.students === 1 && st && st.size > 3000, JSON.stringify(ex));
  await fs.rm(out, { force: true });

  // ── 5. 방과후 요일: 월 7·8교시, 화·목 8교시 ──
  await app.nav("시간표");
  await sleep(600);
  await app.evaluate(`document.querySelector('.tt-bell-btn').click(), true`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.after-school')`), { label: "일과 시간 설정" });
  // 정규 수업은 6교시까지, 방과후를 7·8교시로
  await app.evaluate(`window.__e2e.setValue([...document.querySelectorAll('.bell-grid label')].find(l => l.innerText.includes('교시 수')).querySelector('select'), '6')`);
  await sleep(400);
  await app.click("방과후 추가", ".bell button");
  await app.click("방과후 추가", ".bell button");
  await waitFor(() => app.evaluate(`document.querySelectorAll('.after-row').length === 2`), { label: "방과후 두 줄" });
  const rowSet = async (i, name, start, end, offDays) => {
    await app.evaluate(`(() => {
      const row = document.querySelectorAll('.after-row')[${i}];
      window.__e2e.setValue(row.querySelector('input:not([type])'), ${JSON.stringify(name)});
      const times = row.querySelectorAll('input[type=time]');
      window.__e2e.setValue(times[0], ${JSON.stringify(start)});
      window.__e2e.setValue(times[1], ${JSON.stringify(end)});
      return true; })()`);
    for (const d of offDays) {
      await app.evaluate(`[...document.querySelectorAll('.after-row')[${i}].querySelectorAll('.day-check')].find(l => l.innerText.trim() === ${JSON.stringify(d)}).querySelector('input').click()`);
      await sleep(150);
    }
  };
  await rowSet(0, "7교시", "15:10", "15:55", ["화", "수", "목", "금"]);
  await rowSet(1, "8교시", "16:05", "16:50", ["수", "금"]);
  await sleep(400);
  const checked = await app.evaluate(`[...document.querySelectorAll('.after-row')].map(r => [...r.querySelectorAll('.day-check.on')].map(l => l.innerText.trim()).join(''))`);
  check("방과후 줄마다 요일 체크 (7교시: 월 / 8교시: 월화목)", JSON.stringify(checked) === JSON.stringify(["월", "월화목"]), JSON.stringify(checked));
  const clashWarn = await app.evaluate(`(document.querySelector('.bell')?.innerText || '').includes('정규 수업')`);
  check("정규 6교시 뒤 방과후는 겹침 경고 없음", !clashWarn, "");
  await app.screenshot(path.join(shots, "84-after-school-days.png"));
  await app.click("일과 시간 저장", ".bell button");
  await sleep(600);
  const bell = await app.invoke("get_bell", { year: sem.school_year, semester: sem.semester });
  const after = bell.periods.filter((p) => p.kind === "after").map((p) => `${p.label}:${(p.days ?? []).join(",")}`);
  check("저장: 방과후 요일", JSON.stringify(after) === JSON.stringify(["7교시:1", "8교시:1,2,4"]), JSON.stringify(after));
  await app.click("개인 시간표", ".seg.tabs button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.tt-grid tr.tt-after')`), { label: "방과후 줄" });
  const offs = await app.evaluate(`[...document.querySelectorAll('.tt-grid tr.tt-after')].map(tr => [...tr.querySelectorAll('td.tt-cell')].map(td => td.classList.contains('tt-off') ? 'X' : 'O').join(''))`);
  check("주간 시간표: 방과후가 없는 요일 칸은 막힘", JSON.stringify(offs) === JSON.stringify(["OXXXX", "OOXOX"]), JSON.stringify(offs));
  await app.evaluate(`document.querySelector('.tt-grid tr.tt-after').scrollIntoView({ block: 'center' }); true`);
  await sleep(200);
  await app.screenshot(path.join(shots, "85-after-school-week.png"));

  // ── 6. 다가오는 일정의 점: 학사력 층은 빨강이 아니고, 휴일·휴업일·방학만 날짜 색 ──
  const plus = (n) => iso(new Date(now.getFullYear(), now.getMonth(), now.getDate() + n));
  await app.invoke("create_event", { input: { layer: "annual", title: "1차 지필평가", start_date: plus(3) } });
  await app.invoke("create_event", { input: { layer: "annual", title: "개교기념일", start_date: plus(4) } });
  await app.nav("일정표");
  await sleep(300);
  await app.nav("오늘");
  const dots = await waitFor(async () => {
    const r = await app.evaluate(`[...document.querySelectorAll('#today-briefing .agenda-row')].map(li => ({ t: li.querySelector('.agenda-title')?.innerText.trim(), c: getComputedStyle(li.querySelector('.layer-dot')).backgroundColor, tag: li.querySelector('.day-kind-tag')?.innerText.trim() ?? '' }))`);
    return r.some((x) => x.t === "개교기념일") && r.some((x) => x.t === "1차 지필평가") ? r : null;
  }, { label: "다가오는 일정" });
  const isRed = (c) => {
    const [r, g, b] = (c.match(/\d+/g) ?? []).map(Number);
    return r > 150 && g < 100 && b < 100;
  };
  const exam = dots.find((x) => x.t === "1차 지필평가");
  const closure = dots.find((x) => x.t === "개교기념일");
  check("다가오는 일정: 시험 옆 점은 빨강이 아님 (학사력 층 색)", !!exam && !isRed(exam.c) && exam.tag === "", JSON.stringify(exam));
  check("다가오는 일정: 휴업일만 휴일 색 점 + ‘휴업일’ 표시", !!closure && isRed(closure.c) && closure.tag === "휴업일", JSON.stringify(closure));
  await app.evaluate(`document.querySelector('#today-briefing h3').scrollIntoView({ block: 'center' }); true`);
  await sleep(200);
  await app.screenshot(path.join(shots, "86-upcoming-dots.png"));

  // ── 7. 학사력 달력은 일요일부터 ──
  await app.nav("일정표");
  await sleep(300);
  await app.click("학사력", ".page-head .seg button");
  await waitFor(() => app.evaluate(`document.querySelectorAll('.mini-month').length === 12`), { label: "학사력" });
  const yr = await app.evaluate(`(() => {
    const ms = [...document.querySelectorAll('.mini-month')];
    return ms.map(m => {
      const cells = [...m.querySelector('.mini-grid').children].filter(x => !x.classList.contains('mini-wd'));
      const first = cells.findIndex(x => !x.classList.contains('blank'));
      const sun = cells.find((x, i) => i % 7 === 0 && !x.classList.contains('blank'));
      return { title: m.querySelector('.mini-title').innerText.trim(), heads: [...m.querySelectorAll('.mini-wd')].map(x => x.innerText.trim()).join(''), first, sunDay: sun ? getComputedStyle(sun).color : '' };
    });
  })()`);
  // 학년도(3월 시작)로 보므로 첫 달은 이번 학년도 3월
  const sy = now.getMonth() >= 2 ? now.getFullYear() : now.getFullYear() - 1;
  const firstOk = yr.every((m, i) => m.first === new Date(sy, 2 + i, 1).getDay());
  check("학사력: 요일 머리글이 일요일부터 (일월화수목금토)", yr.every((m) => m.heads === "일월화수목금토"), yr[0]?.heads);
  check("학사력: 1일이 제 요일 칸에 (12달 모두)", firstOk, yr.map((m) => `${m.title}:${m.first}`).join(" "));
  check("학사력: 일요일 날짜는 휴일 색", yr.every((m) => m.sunDay === "rgb(217, 48, 37)"), yr[0]?.sunDay);
  await app.screenshot(path.join(shots, "87-year-sunday-first.png"));

  // ── 8. 왼쪽 메뉴 ‘설정’ 바로 아래: Windows 시작 시 자동 실행, 항상 위에 표시 ──
  // 시험(TN_DATA_ROOT)에서는 실제 시작 프로그램 목록 대신 시험용 키에 쓴다 — 실제 목록은 그대로인지도 확인
  const regValue = (key) => {
    try {
      return execFileSync("reg", ["query", key, "/v", "LocalTeacherNotebookV2"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    } catch {
      return null;
    }
  };
  const REAL_RUN = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
  const TEST_RUN = "HKCU\\Software\\LocalTeacherNotebookV2-Test\\Run";
  const realBefore = regValue(REAL_RUN);
  const opts = await app.evaluate(`(() => {
    const o = document.getElementById('side-opts');
    const prev = o?.previousElementSibling;
    return { labels: [...o.querySelectorAll('label.side-check')].map(l => l.innerText.trim()), afterMenu: prev?.tagName === 'UL' && prev.lastElementChild.innerText.trim() === '설정', checked: [...o.querySelectorAll('input[type=checkbox]')].map(i => i.checked) };
  })()`);
  check("왼쪽 메뉴 ‘설정’ 바로 아래 체크칸 2개 (처음엔 꺼짐)", opts.afterMenu && opts.labels.join("|") === "윈도우 시작 시 자동 실행|항상 위에 표시" && opts.checked.join() === "false,false", JSON.stringify(opts));
  await app.evaluate(`document.querySelectorAll('#side-opts input')[0].click()`);
  const auto = await waitFor(async () => ((await app.invoke("app_info")).autostart ? true : null), { label: "자동 실행 켜짐", timeout: 8000 }).catch(() => false);
  const testVal = regValue(TEST_RUN) ?? "";
  check("자동 실행 켜기 → 창을 띄우는 명령으로 등록 (--background 없음)", auto && testVal.includes(".exe\"") && !testVal.includes("--background"), testVal.split("\n").find((l) => l.includes("REG_SZ"))?.trim() ?? "");
  check("시험 중 실제 시작 프로그램 목록은 그대로", regValue(REAL_RUN) === realBefore, "");
  await app.evaluate(`document.querySelectorAll('#side-opts input')[1].click()`);
  await waitFor(async () => ((await app.invoke("get_settings")).always_on_top ? true : null), { label: "항상 위 켜짐", timeout: 8000 });
  const topmost = () =>
    execFileSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        `Add-Type -Name W -Namespace U -MemberDefinition '[DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);'; $p = Get-Process -Id ${child.pid}; [bool]([U.W]::GetWindowLong($p.MainWindowHandle, -20) -band 8)`,
      ],
      { encoding: "utf8" },
    ).trim() === "True";
  check("항상 위에 표시 켜기 → 창이 맨 위", topmost(), "");
  await sleep(300);
  await app.screenshot(path.join(shots, "88-side-options.png"));
  // 다른 설정을 저장해도(화면에 남은 예전 값) 항상 위는 그대로
  const stale = await app.invoke("get_settings");
  await app.invoke("update_settings", { value: { ...stale, always_on_top: false, homeroom: "1-1" } });
  check("다른 설정을 저장해도 항상 위는 유지", (await app.invoke("get_settings")).always_on_top === true && topmost(), "");
  await app.evaluate(`document.querySelectorAll('#side-opts input')[0].click()`);
  await waitFor(async () => ((await app.invoke("app_info")).autostart ? null : true), { label: "자동 실행 꺼짐", timeout: 8000 });
  await app.evaluate(`document.querySelectorAll('#side-opts input')[1].click()`);
  await waitFor(async () => ((await app.invoke("get_settings")).always_on_top ? null : true), { label: "항상 위 꺼짐", timeout: 8000 });
  check("둘 다 끄기 → 등록 지움·창도 보통으로", regValue(TEST_RUN) === null && !topmost(), "");
  const setText = await (async () => {
    await app.nav("설정");
    await waitFor(() => app.evaluate(`!!document.getElementById('set-notify')`), { label: "설정" });
    return app.text("#set-notify");
  })();
  check("설정 → 알림의 자동 실행 문구", setText.includes("Windows에 로그인하면 교무수첩을 자동으로 실행"), "");

  // ── 업데이트 기록 ──
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.changelog')`), { label: "업데이트 기록" });
  const log = await app.text(".dialog");
  check("업데이트 기록: v1.0 기능 소개 (업무 요약·학생부 자료)", log.includes("v1.0") && log.includes("업무 요약") && log.includes("학생부 자료") && log.includes("최종안"), "");
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  // 시험이 만든 빈 시험용 키 정리 (실제 시작 프로그램 목록과는 다른 키)
  try {
    execFileSync("reg", ["delete", "HKCU\\Software\\LocalTeacherNotebookV2-Test", "/f"], { stdio: "ignore" });
  } catch {}
  await fs.writeFile(path.join(shots, "results-v09.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
