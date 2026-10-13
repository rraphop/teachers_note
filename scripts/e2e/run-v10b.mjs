// 1.0 2차 수정목록 E2E: 메뉴 이름(학생 명렬표·교무 업무), 업데이트 기록(v1.0 기능 소개만), 학급명렬표 열 너비·왼쪽 고정,
//   할 일 칸 폭, 이름·학급 찾기(두 보기), 학생부 자료 학급 펼침·출결 탭, 기록 속 개인정보 → 학생 명렬표(확인·4개·특이사항 질문·가리기),
//   새 항목 숫자(직접 만든 항목은 세지 않음).
//   gguf 를 주면 AI 로 정리하며 원형 진행 표시와 새 항목 숫자·'새' 표시·점 표시까지 본다.
// 학생 이름·주소·번호는 모두 가짜(홍길동 등, OO시 OO구)다.
// 실행: node scripts/e2e/run-v10b.mjs <exe> <데이터폴더> <스크린샷폴더> [gguf 경로]
import fs from "node:fs/promises";
import path from "node:path";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [exe, dataRoot, shots, gguf = ""] = process.argv.slice(2);
await fs.mkdir(shots, { recursive: true });
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "OK " : "NG "} ${name}${detail ? " — " + detail : ""}`);
};
const pad = (n) => String(n).padStart(2, "0");
const iso = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const now = new Date();
const plus = (n) => iso(new Date(now.getFullYear(), now.getMonth(), now.getDate() + n));
const near = (a, b, tol = 1) => Math.abs(a - b) <= tol;

const view = async (app, width, height = 1000) => {
  await app.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await sleep(500);
};

const child = launch(exe, { dataRoot });
let app;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  const info = await app.invoke("app_info");
  check("버전 1.0.x", /^1\.0\.\d+$/.test(info.version), info.version);

  // ── 준비: 가짜 학생 (1-1 셋, 1-2 둘, 2-1 둘), 담임반 1-2 ──
  const roster = [
    ["1-1", 1, "홍길동"],
    ["1-1", 2, "성춘향"],
    ["1-1", 3, "이몽룡"],
    ["1-2", 1, "임꺽정"],
    ["1-2", 2, "전우치"],
    ["2-1", 1, "심청"],
    ["2-1", 2, "흥부"],
  ];
  const ids = {};
  for (const [c, n, name] of roster) ids[name] = await app.invoke("create_student", { input: { name, class_label: c, number: n } });
  const st0 = await app.invoke("get_settings");
  await app.invoke("update_settings", { value: { ...st0, homeroom: "1-2" } });
  const student = async (name) => (await app.invoke("list_students", { query: null, classId: null })).find((s) => s.id === ids[name]);

  // ── 7·3-1. 왼쪽 메뉴 이름 ──
  const menus = await app.evaluate(`[...document.querySelectorAll('.sidebar button')].map(b => b.innerText.replace(/\\s+/g, ' ').trim())`);
  check("왼쪽 메뉴: ‘학생 명렬표’", menus.some((m) => m.startsWith("학생 명렬표")), menus.join(" | "));
  check("왼쪽 메뉴: ‘교무 업무’", menus.some((m) => m.startsWith("교무 업무")) && !menus.some((m) => /^업무(\s|$)/.test(m)), "");

  // ── 4. 업데이트 기록: 출시용 v1.0 기능 소개만 ──
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.changelog')`), { label: "업데이트 기록" });
  const log = await app.text(".dialog");
  check("업데이트 기록: v1.0 하나, 개발 버전(v0.x) 없음", log.includes("v1.0") && !/v0\.\d/.test(log), "");
  check("  기능 소개: 출결·학생 명렬표·교무 업무", log.includes("출결") && log.includes("학생 명렬표") && log.includes("교무 업무"), "");
  await app.screenshot(path.join(shots, "a0-changelog.png"));
  await app.evaluate(`document.querySelector('.dialog button')?.click()`);
  await app.evaluate(`document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))`);
  await sleep(300);

  // ── 1. 학급명렬표 열 너비 ──
  await view(app, 1920);
  await app.nav("학생 명렬표");
  await waitFor(() => app.evaluate(`!!document.querySelector('.student-search')`), { label: "학생 화면" });
  await app.click("학급명렬표", ".seg button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.roster-table') && !!document.querySelector('.roster-cols input:not([disabled])')`), { label: "학급명렬표" });
  const setCols = async (labels) => {
    await waitFor(() => app.evaluate(`!!document.querySelector('.roster-cols input:not([disabled])')`), { label: "보일 항목 준비", timeout: 6000 });
    await app.evaluate(`(() => {
      const want = ${JSON.stringify(labels)};
      for (const l of document.querySelectorAll('.roster-cols label.check')) {
        const box = l.querySelector('input');
        const on = want.includes(l.innerText.trim());
        if (box.checked !== on) box.click();
      }
      return true;
    })()`);
    await waitFor(
      () =>
        app.evaluate(`(() => {
          const heads = [...document.querySelectorAll('.roster-table thead th')].filter(th => getComputedStyle(th).display !== 'none').map(th => th.innerText.trim());
          return heads.length === ${labels.length + 4};
        })()`),
      { label: "보일 항목 반영", timeout: 6000 },
    );
    await sleep(300);
  };
  const measure = () =>
    app.evaluate(`(() => {
      const box = document.querySelector('.roster-scroll');
      const heads = [...document.querySelectorAll('.roster-table thead th')].filter(th => getComputedStyle(th).display !== 'none').map(th => {
        const b = th.getBoundingClientRect();
        return { t: th.innerText.trim(), l: Math.round(b.left), r: Math.round(b.right), w: Math.round(b.width) };
      });
      return { heads, boxW: box.clientWidth, scrollW: box.scrollWidth, boxL: Math.round(box.getBoundingClientRect().left) };
    })()`);
  const toggleMask = async () => {
    const box = `[...document.querySelectorAll('.roster label.check')].find(l => l.innerText.includes('연락처·주소 가리기'))?.querySelector('input')`;
    await waitFor(() => app.evaluate(`!!${box}`), { label: "가리기 칸", timeout: 6000 });
    await app.evaluate(`${box}.click(), true`);
  };
  const col = (m, t) => m.heads.find((h) => h.t.startsWith(t));
  const last = (m) => m.heads[m.heads.length - 1];

  await setCols([]);
  let m = await measure();
  check(
    // 3차 1번에서 "남는 폭은 오른쪽 여백" 규칙이 폐기됨: 이제 네 칸이 비율대로 표 폭을 채운다 (자세한 검사는 run-v10c.mjs)
    "선택 항목 0개: 학급·번호·이름·기록 네 칸, 기록은 이름 바로 옆",
    m.heads.map((h) => h.t).join(",") === "학급,번호,이름,기록" && near(col(m, "기록").l, col(m, "이름").r),
    JSON.stringify(m.heads.map((h) => [h.t, h.w])),
  );
  check("  표가 폭을 채우고 가로 스크롤 없음 (3차 1번)", m.scrollW <= m.boxW && near(last(m).r - m.boxL, m.boxW, 3), `표 끝 ${last(m).r - m.boxL} / 칸 ${m.boxW}`);

  await setCols(["전화번호", "비고"]);
  for (const w of [1920, 1280]) {
    await view(app, w);
    m = await measure();
    const phone = col(m, "전화번호");
    const remark = col(m, "비고");
    const rec = col(m, "기록");
    check(
      `전화번호+비고 (${w}px): 기록 칸이 비고 바로 옆(맨 오른쪽), 비고가 전화번호보다 넓음`,
      last(m).t === "기록" && near(rec.l, remark.r) && remark.w > phone.w,
      JSON.stringify(m.heads.map((h) => [h.t, h.w])),
    );
    check(`  ${w}px: 가로 스크롤 없음`, m.scrollW <= m.boxW, `${m.scrollW}/${m.boxW}`);
  }
  await app.screenshot(path.join(shots, "a1-roster-phone-remark.png"));

  await setCols(["전화번호", "보호자 연락처", "주소", "성별", "생일", "특이사항", "학생개인번호", "이전학적", "비고"]);
  m = await measure();
  check("전체 켬 (1280px): 가로 스크롤 생김, 기록 칸은 마지막 선택 칸 바로 옆", m.scrollW > m.boxW && last(m).t === "기록" && near(last(m).l, m.heads[m.heads.length - 2].r), `${m.scrollW}/${m.boxW}`);
  const pinned = () =>
    app.evaluate(`(() => {
      const box = document.querySelector('.roster-scroll');
      const tr = document.querySelector('.roster-table tbody tr:not(.add-row)');
      const bl = box.getBoundingClientRect().left;
      return [...tr.children].slice(0, 4).map(td => Math.round(td.getBoundingClientRect().left - bl));
    })()`);
  const before = await pinned();
  await app.evaluate(`document.querySelector('.roster-scroll').scrollLeft = 500, true`);
  await sleep(300);
  const after = await pinned();
  check(
    "  가로로 밀어도 학급·번호·이름은 왼쪽에 고정, 다음 칸은 밀림",
    before.slice(0, 3).every((x, i) => near(x, after[i])) && after[3] < before[3] - 100,
    `${JSON.stringify(before)} → ${JSON.stringify(after)}`,
  );
  await app.screenshot(path.join(shots, "a2-roster-all-scrolled.png"));
  await app.evaluate(`document.querySelector('.roster-scroll').scrollLeft = 0, true`);
  // 가리기를 켜도 같은 규칙
  await toggleMask();
  await setCols(["전화번호", "비고"]);
  m = await measure();
  check("가리기 켬: 전화번호+비고 규칙 그대로 (기록이 비고 옆)", near(col(m, "기록").l, col(m, "비고").r) && m.scrollW <= m.boxW, JSON.stringify(m.heads.map((h) => [h.t, h.w])));
  await toggleMask();
  await view(app, 1920);

  // ── 9. 이름·학급 찾기: 두 보기 모두 ──
  const rows = () => app.evaluate(`[...document.querySelectorAll('.roster-table tbody tr:not(.add-row)')].map(tr => tr.children[2].innerText.trim())`);
  await app.type(".student-search", "춘향");
  await waitFor(async () => ((await rows()).length === 1 ? true : null), { label: "찾기 결과", timeout: 5000 }).catch(() => null);
  const found = await rows();
  const marked = await app.evaluate(`[...document.querySelectorAll('.roster-table tbody mark')].map(x => x.innerText).join(',')`);
  const scope = await app.evaluate(`[...document.querySelectorAll('.roster-bar button')].map(b => b.innerText).filter(t => t.includes('검색 결과')).join(' | ')`);
  check("학급명렬표에서 찾기: 맞는 줄만, 이름의 맞는 글자 강조", found.length === 1 && found[0] === "성춘향" && marked === "춘향", `${found} · 강조 ${marked}`);
  check("  [엑셀로 저장]·[인쇄]에 ‘검색 결과 1명’", (scope.match(/검색 결과 1명/g) ?? []).length === 2, scope);
  await app.type(".student-search", "1학년");
  await sleep(300);
  await app.click("1-1", ".class-filter button");
  await sleep(500);
  const r11 = await rows();
  check("‘1학년’ + 학급 1-1 버튼: 1-1 학생만", r11.length === 3 && r11.every((n) => ["홍길동", "성춘향", "이몽룡"].includes(n)), r11.join(","));
  await app.click("전체 학급", ".class-filter button");
  await sleep(500);
  check("  전체 학급: 1학년 다섯 명", (await rows()).length === 5, (await rows()).join(","));
  await app.click("카드", ".seg button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.student-grid')`), { label: "카드 보기" });
  const kept = await app.evaluate(`document.querySelector('.student-search')?.value`);
  const cards = await app.evaluate(`document.querySelectorAll('.student-card').length`);
  check("카드로 바꿔도 같은 자리 찾기 칸, 검색어 유지", kept === "1학년" && cards === 5, `${kept} · 카드 ${cards}`);
  await app.type(".student-search", "1-2");
  await sleep(300);
  check("  ‘1-2’ 학급 표기로 찾기", (await app.evaluate(`document.querySelectorAll('.student-card').length`)) === 2, "");
  await app.type(".student-search", "심청");
  await sleep(300);
  await app.evaluate(`document.querySelector('.student-search').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
  await waitFor(() => app.evaluate(`[...document.querySelectorAll('.page-head h1')].some(h => h.innerText.includes('심청')) && !document.querySelector('.student-search')`), { label: "학생 상세", timeout: 5000 })
    .then(() => check("결과가 하나면 Enter 로 학생 상세 열기", true))
    .catch(() => check("결과가 하나면 Enter 로 학생 상세 열기", false));
  await app.click("학생 목록", ".page-head button");
  await sleep(300);
  await app.type(".student-search", "");

  // ── 2. 할 일 칸 폭 ──
  await app.invoke("create_task", { input: { title: "2학기 수행평가 채점 기준표를 교과협의회 자료로 정리해 교무부에 제출하기", due_date: plus(0) } });
  await app.invoke("create_task", { input: { title: "가정통신문 회수", due_date: plus(2) } });
  await app.invoke("create_task", { input: { title: "교실 환경 정리", due_date: null } });
  await app.nav("할 일");
  await waitFor(() => app.evaluate(`!!document.querySelector('.bucket-grid .agenda-title')`), { label: "할 일" });
  const tasksAt = () =>
    app.evaluate(`(() => {
      const g = document.querySelector('.bucket-grid');
      const main = document.querySelector('.main');
      const panels = [...g.querySelectorAll(':scope > .panel')];
      const tops = new Set(panels.map(p => Math.round(p.getBoundingClientRect().top)));
      const lefts = new Set(panels.map(p => Math.round(p.getBoundingClientRect().left)));
      const long = [...document.querySelectorAll('.agenda-title')].find(t => t.innerText.includes('채점 기준표'));
      const lh = parseFloat(getComputedStyle(long).lineHeight) || 22;
      return { cols: lefts.size, rows: tops.size, rightGap: Math.round(main.getBoundingClientRect().right - g.getBoundingClientRect().right), hScroll: document.documentElement.scrollWidth > document.documentElement.clientWidth || main.scrollWidth > main.clientWidth, oneLine: long.getBoundingClientRect().height < lh * 1.6 };
    })()`);
  await view(app, 1920);
  let t = await tasksAt();
  check("할 일 (1920px): 본문 폭을 다 채움(오른쪽 빈 곳 없음), 긴 제목 한 줄", t.cols === 1 && t.rightGap <= 40 && t.oneLine && !t.hScroll, JSON.stringify(t));
  await app.screenshot(path.join(shots, "a3-tasks-1920.png"));
  await view(app, 2560);
  t = await tasksAt();
  check("할 일 (2560px, 아주 넓음): 묶음 2열 이상 나란히, 긴 제목 한 줄", t.cols >= 2 && t.rightGap <= 40 && t.oneLine && !t.hScroll, JSON.stringify(t));
  await view(app, 760);
  t = await tasksAt();
  check("할 일 (760px): 1열, 가로 스크롤 없음", t.cols === 1 && !t.hScroll, JSON.stringify(t));
  await view(app, 1920);

  // ── 3-2·3-3. 학생부 자료: 학급 펼침, 출결 탭 ──
  for (const [content, d] of [
    ["감기로 병원 진료 결석", -6],
    ["연락 없이 지각", -4],
    ["체험학습으로 2일 결석", -2],
  ])
    await app.invoke("create_observation", { input: { student_id: ids["홍길동"], area: "attendance", content, observed_on: plus(d) } });
  await app.evaluate(`(() => { try { localStorage.removeItem('tn.records.class'); localStorage.removeItem('tn.records.area'); } catch {} return true; })()`);
  await app.nav("학생부 자료");
  await waitFor(() => app.evaluate(`document.querySelectorAll('.pick-group').length === 3`), { label: "학급 목록" });
  const groups = () =>
    app.evaluate(`[...document.querySelectorAll('.pick-group')].map(g => ({ label: g.querySelector('.pick-class').innerText.replace(/\\s+/g, ' ').trim(), open: g.classList.contains('open'), n: g.querySelectorAll('li').length }))`);
  let gs = await waitFor(async () => {
    const x = await groups();
    return x.some((g) => g.open) ? x : null;
  }, { label: "펼친 학급", timeout: 5000 });
  check("처음엔 담임반(1-2)만 펼침", gs.filter((g) => g.open).length === 1 && gs.find((g) => g.open).label.includes("1-2"), JSON.stringify(gs));
  check("  접힌 학급 제목줄에도 ‘최종안 0/3’", gs.find((g) => g.label.includes("1-1")).label.includes("최종안 0/3") && gs.find((g) => g.label.includes("1-1")).n === 0, "");
  await app.click("1-1", ".pick-class");
  await sleep(300);
  gs = await groups();
  check("다른 학급을 펼치면 앞 학급은 접힘 (한 번에 한 학급)", gs.filter((g) => g.open).length === 1 && gs.find((g) => g.open).label.includes("1-1"), JSON.stringify(gs.map((g) => [g.label, g.open])));
  await app.nav("할 일");
  await sleep(300);
  await app.nav("학생부 자료");
  await waitFor(() => app.evaluate(`document.querySelectorAll('.pick-group').length === 3`), { label: "학급 목록" });
  await sleep(300);
  gs = await groups();
  check("다시 와도 마지막으로 보던 학급(1-1)을 펼침", gs.find((g) => g.open)?.label.includes("1-1"), JSON.stringify(gs.map((g) => [g.label, g.open])));
  await app.type(".student-pick .search-input", "흥부");
  await sleep(200);
  await app.evaluate(`document.querySelector('.student-pick .search-input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
  await waitFor(() => app.evaluate(`(document.querySelector('.student-title')?.innerText || '').includes('흥부')`), { label: "흥부", timeout: 5000 }).catch(() => null);
  gs = await groups();
  check("학생 찾기 Enter: 학급과 상관없이 그 학생으로 이동, 그 학급 펼침", (await app.text(".student-title")).includes("흥부") && gs.find((g) => g.open)?.label.includes("2-1"), JSON.stringify(gs.map((g) => [g.label, g.open])));
  const areaTabs = await app.evaluate(`[...document.querySelectorAll('.area-tabs button')].map(b => b.innerText.trim())`);
  check("영역 탭: 출결이 맨 앞 (출결 → 창체 …)", areaTabs[0] === "출결" && areaTabs.length >= 8, areaTabs.join(","));
  await app.click("1-1", ".pick-class");
  await sleep(200);
  await app.click("홍길동", ".student-pick li button");
  await app.click("출결", ".area-tabs button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.att-table')`), { label: "출결 요약", timeout: 8000 });
  const att = await app.evaluate(`Object.fromEntries([...document.querySelectorAll('.att-table tbody tr')].map(tr => [tr.querySelector('th').innerText, [...tr.querySelectorAll('td')].map(td => td.innerText.trim())]))`);
  check(
    "출결 탭: 결석·지각을 사유별로 집계 (질병 1·출석 인정 2 → 결석 3, 미인정 지각 1)",
    JSON.stringify(att["결석"]) === JSON.stringify(["1", "", "", "2", "3"]) && JSON.stringify(att["지각"]) === JSON.stringify(["", "1", "", "", "1"]),
    JSON.stringify(att),
  );
  const recText = await app.text(".records-main");
  check("  근거 기록 3건과 ‘출결상황 특기사항 최종안’ 입력 칸, 글자 수 한도", recText.includes("근거 기록") && recText.includes("출결상황 특기사항 최종안") && (await app.evaluate(`document.querySelectorAll('.ref-list > li').length`)) === 3 && /최대 \d+자/.test(recText), "");
  check("  학생 목록 상태: 홍길동 ○ (참고자료만)", await app.evaluate(`[...document.querySelectorAll('.student-pick li button')].find(b => b.innerText.includes('홍길동'))?.querySelector('.rec-mark')?.innerText === '○'`), "");
  await app.screenshot(path.join(shots, "a4-records-attendance.png"));

  // ── 8. 기록 속 개인정보 → 학생 명렬표 (AI 없이 규칙 제안을 저장하는 흐름) ──
  const ruleNote = async (raw) => {
    const n = await app.invoke("create_note", { raw });
    await waitFor(async () => ((await app.invoke("get_note_detail", { id: n.id })).note.status !== "pending" ? true : null), { label: "원문 저장", timeout: 15000 }).catch(() => null);
    const plan = await app.invoke("suggest_plan", { id: n.id });
    const note = await app.invoke("save_plan", { id: n.id, plan });
    return { id: n.id, plan, note };
  };
  let r = await ruleNote("1-1 홍길동 주소 OO시 OO구 OO로 12");
  let s = await student("홍길동");
  check(
    "주소 → 명렬표 주소 칸 (행특·미지정으로 가지 않음)",
    s.address === "OO시 OO구 OO로 12" && (r.plan.profile ?? []).some((p) => p.field === "address") && !r.plan.items.some((i) => i.kind === "student" || i.kind === "task"),
    `${s.address} · 항목 ${r.plan.items.map((i) => i.kind).join(",")}`,
  );
  check("  기록 카드 태그 ‘학생 명렬표 · 주소’", r.note.profile.some((p) => p.status === "applied" && p.field_label === "주소"), "");
  r = await ruleNote("1-1 홍길동 전화번호 010-1234-5678");
  s = await student("홍길동");
  check("전화번호 → 명렬표 전화번호", s.phone === "010-1234-5678", s.phone);
  r = await ruleNote("1-1 홍길동 어머니 연락처 010-9876-5432");
  s = await student("홍길동");
  check("어머니 연락처 → 보호자 연락처 (관계: 모)", s.contacts.length === 1 && s.contacts[0].relation === "모" && s.contacts[0].phone === "010-9876-5432", JSON.stringify(s.contacts.map((c) => c.relation)));
  for (const raw of ["1-1 홍길동 아버지 010-2222-3333", "1-1 홍길동 할머니 010-4444-5555", "1-1 홍길동 삼촌 010-6666-7777"]) await ruleNote(raw);
  s = await student("홍길동");
  check("아버지·할머니·삼촌 → 연락처 4개", s.contacts.length === 4, s.contacts.map((c) => c.relation).join(","));
  const fifth = await ruleNote("1-1 홍길동 이모 010-8888-9999");
  s = await student("홍길동");
  check("다섯 번째 번호: 바로 넣지 않고 확인 대기", s.contacts.length === 4 && fifth.note.profile.some((p) => p.status === "pending" && p.target_id === null), "");
  const sameRel = await ruleNote("1-1 홍길동 어머니 번호 바뀜 010-1111-2222");
  check("같은 관계(모)의 새 번호: 추가하지 않고 ‘기존 → 새 값’ 확인 대기", sameRel.note.profile.some((p) => p.status === "pending" && p.target_id !== null && p.old_value === "010-9876-5432"), "");
  await ruleNote("1-1 홍길동 생일 3월 5일");
  s = await student("홍길동");
  check("생일 3월 5일 → 생일 칸", (s.birthday ?? "").endsWith("03-05"), s.birthday);
  const addr2 = await ruleNote("1-1 홍길동 주소 OO시 OO구 OO로 34");
  s = await student("홍길동");
  check("주소가 이미 있는 학생에게 새 주소: 덮어쓰지 않고 확인 대기", s.address === "OO시 OO구 OO로 12" && addr2.note.profile.some((p) => p.status === "pending" && p.field === "address"), s.address);
  const special = await ruleNote("1-1 성춘향 수업 중 자주 어지러움 호소");
  check("특이사항성 내용 → ‘어디에 둘까요?’ 질문", special.note.questions.some((q) => q.status === "pending"), "");
  const ov = await app.invoke("today_overview");
  check("  답할 것(질문 1 + 확인 3)이 기록 메뉴 숫자에 포함", ov.needs_answer_count === 4, `확인 필요 ${ov.needs_answer_count}`);

  // 화면에서 답하기
  await app.nav("기록");
  await waitFor(() => app.evaluate(`!!document.querySelector('#note-${special.id}')`), { label: "기록 화면", timeout: 8000 });
  const badgeOf = `[...document.querySelectorAll('.sidebar button')].find(b => b.innerText.startsWith('기록'))?.querySelector('.nav-badge')?.className || ''`;
  const notesBadge = (await waitFor(() => app.evaluate(badgeOf), { label: "기록 메뉴 숫자", timeout: 6000 }).catch(() => "")) || "";
  check("  기록 메뉴 숫자는 회색(정리 중·확인 필요), 새 항목 숫자(초록)와 다름", notesBadge.includes("pending") && !notesBadge.includes("new"), notesBadge);
  await app.screenshot(path.join(shots, "a5-notes-profile.png"));
  await app.evaluate(`window.__e2e.click('학생 명렬표 특이사항', '#note-${special.id} button')`);
  await waitFor(async () => ((await student("성춘향")).memo.includes("어지러움") ? true : null), { label: "특이사항", timeout: 5000 }).catch(() => null);
  const chun = await student("성춘향");
  const chunRec = await app.invoke("student_record", { studentId: ids["성춘향"] });
  check("[학생 명렬표 특이사항] → 명렬표 특이사항 칸, 학생부 참고자료에는 없음", chun.memo.includes("어지러움") && !chunRec.some((tab) => tab.refs.some((o) => o.content.includes("어지러움"))), chun.memo);
  // 4개가 찼을 때: 바꿀 번호 고르기
  await app.evaluate(`(() => {
    const card = document.querySelector('#note-${fifth.id}');
    const radio = [...card.querySelectorAll('.contact-pick label')].find(l => l.innerText.includes('삼촌')).querySelector('input');
    radio.click();
    return true;
  })()`);
  await sleep(200);
  await app.evaluate(`window.__e2e.click('바꾸기', '#note-${fifth.id} .profile-confirm button')`);
  await waitFor(async () => ((await student("홍길동")).contacts.some((c) => c.relation === "이모") ? true : null), { label: "연락처 바꾸기", timeout: 5000 }).catch(() => null);
  s = await student("홍길동");
  check("4개가 찼을 때 고른 번호(삼촌)를 새 번호(이모)로 바꿈", s.contacts.length === 4 && s.contacts.some((c) => c.relation === "이모") && !s.contacts.some((c) => c.relation === "삼촌"), s.contacts.map((c) => c.relation).join(","));
  await app.evaluate(`window.__e2e.click('바꾸기', '#note-${sameRel.id} .profile-confirm button')`);
  await waitFor(async () => ((await student("홍길동")).contacts.some((c) => c.phone === "010-1111-2222") ? true : null), { label: "같은 관계 바꾸기", timeout: 5000 }).catch(() => null);
  s = await student("홍길동");
  check("같은 관계 [바꾸기] → 모 번호가 새 값으로 (개수 그대로)", s.contacts.length === 4 && s.contacts.find((c) => c.relation === "모")?.phone === "010-1111-2222", "");
  await app.evaluate(`window.__e2e.click('그대로 두기', '#note-${addr2.id} .profile-confirm button')`);
  await sleep(600);
  s = await student("홍길동");
  const det = await app.invoke("get_student_detail", { id: ids["홍길동"] });
  const addrHist = det.profile_history.filter((p) => p.field === "address").map((p) => p.status);
  check("새 주소 [그대로 두기] → 주소 유지, 이력은 ‘그대로 둠’으로 남음", s.address === "OO시 OO구 OO로 12" && JSON.stringify(addrHist) === JSON.stringify(["rejected", "applied"]), JSON.stringify(addrHist));
  check("  답을 다 하면 확인 필요 0", (await app.invoke("today_overview")).needs_answer_count === 0, "");
  // 학생부 자료·AI 초안 문맥에 개인정보 없음
  const hongRec = await app.invoke("student_record", { studentId: ids["홍길동"] });
  const allRefs = hongRec.flatMap((tab) => tab.refs.map((o) => o.content)).join(" ");
  check("학생부 참고자료에 전화번호·주소가 들어가지 않음", !/010-\d{4}-\d{4}/.test(allRefs) && !allRefs.includes("OO로"), "");
  // 가리기: 명렬표에서 켜면 기록 화면에서도 가림
  await app.nav("학생 명렬표");
  await app.click("학급명렬표", ".seg button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.roster-table')`), { label: "학급명렬표" });
  await setCols(["전화번호", "보호자 연락처", "주소", "생일", "특이사항"]);
  const hongRow = () => app.evaluate(`[...document.querySelectorAll('.roster-table tbody tr')].find(tr => tr.children[2]?.innerText.includes('홍길동'))?.innerText || ''`);
  const rowText = await hongRow();
  check("명렬표 보호자 연락처 칸: 첫 번호 + ‘+3’", /\+3/.test(rowText), rowText.replace(/\s+/g, " "));
  await toggleMask();
  await sleep(300);
  const maskedRow = await hongRow();
  check("  가리기 켬: 명렬표의 번호·주소 가림", !maskedRow.includes("1234-5678") && !maskedRow.includes("OO로 12") && maskedRow.includes("●"), "");
  await app.evaluate(`[...document.querySelectorAll('.roster-table tbody tr')].find(tr => tr.children[2]?.innerText.includes('홍길동')).querySelector('td.no-print button').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.profile-history')`), { label: "변경 이력", timeout: 6000 }).catch(() => null);
  const histText = await app.text(".profile-history");
  check(
    "학생 상세: 명렬표 변경 이력 (넣음·그대로 둠, 가리기 적용)",
    histText.includes("넣음") && histText.includes("그대로 둠") && histText.includes("보호자 연락처 (모)") && !histText.includes("1234-5678") && !histText.includes("9876-5432") && histText.includes("●"),
    "",
  );
  await app.screenshot(path.join(shots, "a5b-student-history.png"));
  await app.click("학생 목록", ".page-head button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.roster-table')`), { label: "학급명렬표" });
  await app.nav("기록");
  await waitFor(() => app.evaluate(`!!document.querySelector('.note-card')`), { label: "기록 화면" });
  await sleep(300);
  const notesText = await app.text(".main");
  check("  기록 화면에서도 번호·주소 가림", !notesText.includes("010-1234-5678") && !notesText.includes("9876-5432") && !notesText.includes("OO로 12") && notesText.includes("●●●-●●●●-5678"), "");
  await app.screenshot(path.join(shots, "a6-notes-masked.png"));
  await app.nav("학생 명렬표");
  await waitFor(() => app.evaluate(`!!document.querySelector('.roster-table')`), { label: "학급명렬표" });
  await toggleMask();
  await app.click("카드", ".seg button");

  // ── 5. 새 항목 숫자: 직접 만든 항목은 세지 않음 ──
  const ov2 = await app.invoke("today_overview");
  const greens = await app.evaluate(`document.querySelectorAll('.sidebar .nav-badge.new').length`);
  check("직접 만든 할 일·참고자료·규칙 제안 저장은 새 항목으로 세지 않음", JSON.stringify(ov2.new_counts) === JSON.stringify({ records: 0, duties: 0, tasks: 0, students: 0 }) && greens === 0, JSON.stringify(ov2.new_counts));

  // ── 6·5. AI 로 정리: 원형 진행 표시, 새 항목 숫자·'새'·점 ──
  if (gguf) {
    await app.invoke("import_model_file", { path: gguf });
    await app.nav("오늘");
    await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "입력창" });
    // 원형 게이지 상태를 계속 적어 둔다
    await app.evaluate(`(() => {
      window.__ring = [];
      const seen = () => {
        const el = document.querySelector('.ai-ring');
        const label = el ? el.querySelector('.ai-ring-label')?.innerText.replace(/\\s+/g, ' ').trim() : '';
        const prev = window.__ring[window.__ring.length - 1];
        if ((prev ?? '') !== label) window.__ring.push(label);
      };
      new MutationObserver(seen).observe(document.body, { subtree: true, childList: true, characterData: true });
      return true;
    })()`);
    const pos = await app.evaluate(`(() => {
      const ring = document.querySelector('.capture .ai-ring');
      return ring ? 'ring' : 'none';
    })()`);
    check("정리할 것이 없으면 원형 게이지 숨김", pos === "none", pos);
    const typeAndSave = async (text) => {
      // 앞 기록을 저장하는 동안([저장 중…])은 기다린다
      await waitFor(() => app.evaluate(`document.querySelector('.capture-send')?.innerText.trim() === '기록'`), { label: "저장 버튼", timeout: 10000 });
      await app.type(".capture textarea", text);
      await app.click("기록", ".capture button");
    };
    await typeAndSave("1-2 임꺽정 다음 주 금요일까지 진로 활동 보고서 제출 받기");
    await typeAndSave("1-2 전우치 과학 실험 시간에 모둠 친구들에게 실험 순서를 차근차근 설명하며 협력을 이끌어 냄");
    const ringPlace = await waitFor(
      () =>
        app.evaluate(`(() => {
          const ring = document.querySelector('.capture .ai-ring');
          if (!ring) return null;
          const btn = [...document.querySelectorAll('.capture button')].find(b => b.innerText.trim() === '기록');
          const rr = ring.getBoundingClientRect(), br = btn.getBoundingClientRect();
          return { leftOfButton: rr.right <= br.left + 2 && Math.abs((rr.top + rr.bottom) / 2 - (br.top + br.bottom) / 2) < 30, svg: !!ring.querySelector('svg circle.ring-fg'), bar: !!ring.querySelector('progress') };
        })()`),
      { label: "원형 게이지", timeout: 20000, interval: 100 },
    );
    check("[기록] 버튼 바로 왼쪽에 원형 게이지 (가로 막대 아님)", ringPlace.leftOfButton && ringPlace.svg && !ringPlace.bar, JSON.stringify(ringPlace));
    await app.screenshot(path.join(shots, "a7-ai-ring.png"));
    await waitFor(async () => ((await app.invoke("today_overview")).unclassified_count === 0 ? true : null), { label: "AI 정리 끝", timeout: 600000, interval: 2000 });
    await sleep(2500);
    const ring = await app.evaluate(`window.__ring`);
    const pcts = ring.map((x) => Number((x.match(/AI 정리 중 (\d+)%/) ?? [])[1])).filter((x) => !Number.isNaN(x));
    check("게이지 문구: ‘AI 정리 중 n%’가 차오름 (엔진 켜는 중 표시 포함)", pcts.length >= 2 && Math.max(...pcts) > Math.min(...pcts) && ring.some((x) => x.includes("AI 켜는 중")), ring.slice(0, 14).join(" → "));
    check("  두 건이면 ‘1건 남음’", ring.some((x) => x.includes("1건 남음")), "");
    check("  끝나면 체크 표시 후 사라짐", ring.some((x) => x.includes("AI 정리 끝")) && ring[ring.length - 1] === "", ring.slice(-3).join(" → "));
    const nc = (await app.invoke("today_overview")).new_counts;
    const badges = await app.evaluate(`Object.fromEntries([...document.querySelectorAll('.sidebar button')].map(b => [b.innerText.replace(/\\d+|99\\+/g, '').trim(), b.querySelector('.nav-badge.new')?.innerText || '']))`);
    check("AI가 넣은 항목 → 메뉴 옆 초록 숫자 (할 일·학생부 자료)", nc.tasks >= 1 && nc.records >= 1 && badges["할 일"] === String(nc.tasks) && badges["학생부 자료"] === String(nc.records), `${JSON.stringify(nc)} · ${JSON.stringify(badges)}`);
    await app.screenshot(path.join(shots, "a8-new-badges.png"));
    await app.nav("할 일");
    await waitFor(() => app.evaluate(`!!document.querySelector('.bucket-grid .new-tag')`), { label: "새 표시", timeout: 8000 })
      .then(() => check("할 일을 열면 새 항목에 ‘새’ 표시", true))
      .catch(() => check("할 일을 열면 새 항목에 ‘새’ 표시", false));
    await waitFor(async () => ((await app.invoke("today_overview")).new_counts.tasks === 0 ? true : null), { label: "숫자 사라짐", timeout: 5000 }).catch(() => null);
    check("  열면 할 일 숫자가 사라짐", (await app.invoke("today_overview")).new_counts.tasks === 0, "");
    await app.nav("학생부 자료");
    await waitFor(() => app.evaluate(`document.querySelectorAll('.student-pick .new-dot, .area-tabs .new-dot').length > 0`), { label: "점 표시", timeout: 8000 }).catch(() => null);
    const dots = await app.evaluate(`({
      cls: [...document.querySelectorAll('.pick-class')].filter(b => b.querySelector('.new-dot')).map(b => b.innerText.trim().split(/\\s/)[1] || b.innerText.trim()),
      area: [...document.querySelectorAll('.area-tabs button')].filter(b => b.querySelector('.new-dot')).map(b => b.innerText.trim()),
      kids: [...document.querySelectorAll('.student-pick li button')].filter(b => b.querySelector('.new-dot')).map(b => b.innerText.replace(/\\s+/g, ' ').trim()),
    })`);
    check("학생부 자료: 학급 제목줄·영역 탭에 점", dots.cls.length >= 1 && dots.area.length >= 1, JSON.stringify(dots));
    await app.click("1-2", ".pick-class");
    await sleep(300);
    const kidDot = await app.evaluate(`[...document.querySelectorAll('.student-pick li button')].filter(b => b.querySelector('.new-dot')).map(b => b.innerText.replace(/[^가-힣]/g, ''))`);
    check("  학생 이름 옆에 점 (전우치)", kidDot.includes("전우치"), kidDot.join(","));
    await app.click("전우치", ".student-pick li button");
    const area = dots.area[0];
    if (area) await app.click(area, ".area-tabs button");
    await sleep(800);
    // 교과는 과목 칸이 나뉘므로 새 참고자료가 있는 과목을 찾아 연다
    const subs = await app.evaluate(`document.querySelectorAll('.sub-tabs button').length`);
    for (let i = 0; i < subs && !(await app.evaluate(`!!document.querySelector('.ref-list .new-tag')`)); i++) {
      await app.evaluate(`document.querySelectorAll('.sub-tabs button')[${i}].click(), true`);
      await sleep(600);
    }
    await waitFor(() => app.evaluate(`!!document.querySelector('.ref-list .new-tag')`), { label: "새 참고자료", timeout: 8000 })
      .then(() => check("  새로 들어온 참고자료에 ‘새’ 표시", true))
      .catch(() => check("  새로 들어온 참고자료에 ‘새’ 표시", false));
    await app.screenshot(path.join(shots, "a9-records-new.png"));
    check("  열면 학생부 자료 숫자가 사라짐", (await app.invoke("today_overview")).new_counts.records === 0, "");
    // 학생 명렬표: AI 가 개인정보를 넣으면 같은 방식
    await app.nav("오늘");
    await typeAndSave("1-2 임꺽정 주소 OO시 OO구 OO로 56");
    await sleep(1500);
    await waitFor(async () => ((await app.invoke("today_overview")).unclassified_count === 0 ? true : null), { label: "AI 정리 끝", timeout: 300000, interval: 2000 });
    const lim = await student("임꺽정");
    const nc2 = (await app.invoke("today_overview")).new_counts;
    check("AI 정리: 주소 → 명렬표, 학생 명렬표 메뉴에 초록 숫자", lim.address === "OO시 OO구 OO로 56" && nc2.students === 1, `${lim.address} · ${JSON.stringify(nc2)}`);
    await app.nav("학생 명렬표");
    await waitFor(() => app.evaluate(`[...document.querySelectorAll('.student-card')].some(c => c.innerText.includes('임꺽정') && c.querySelector('.new-dot'))`), { label: "명렬표 점", timeout: 8000 })
      .then(() => check("  학생 명렬표: 새로 들어간 학생 이름 옆에 점", true))
      .catch(() => check("  학생 명렬표: 새로 들어간 학생 이름 옆에 점", false));
    await app.invoke("runtime_stop").catch(() => null);
  } else {
    console.log("(gguf 경로가 없어 AI 정리 부분은 건너뜀)");
  }
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    await app?.send("Emulation.clearDeviceMetricsOverride");
  } catch {}
  try {
    app?.close();
  } catch {}
  child.kill();
  await fs.writeFile(path.join(shots, "results-v10b.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
