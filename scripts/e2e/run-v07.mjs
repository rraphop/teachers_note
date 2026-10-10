// 0.7 E2E: 나이스 명렬표 가져오기(가짜 파일), 학생 상세의 숨김 정보, 날짜 문장의 제목·시각·메모, 학년·학급 인식,
//          기록 세 탭(모든 기록 / 기타 기록 / 정리된 기록)과 기록 안 검색, 오늘의 기록에서 기타 기록 흐리게, 라이선스 및 저작권
// 실행: node scripts/e2e/run-v07.mjs <exe> <데이터폴더> <스크린샷폴더> <가짜명렬표폴더>
// 가짜 명렬표는 `cargo test --lib neis_roster::tests::write_fake_roster_files -- --ignored` (TN_FAKE_ROSTER_DIR) 로 만든다.
import fs from "node:fs/promises";
import path from "node:path";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [exe, dataRoot, shots, rosterDir] = process.argv.slice(2);
await fs.mkdir(shots, { recursive: true });
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "OK " : "NG "} ${name}${detail ? " — " + detail : ""}`);
};
const pad = (n) => String(n).padStart(2, "0");
const iso = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const now = new Date();
const tmr = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
const tomorrow = iso(tmr);
const files = (names) => names.map((n) => path.resolve(rosterDir, n));

const child = launch(exe, { dataRoot });
let app;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  const info = await app.invoke("app_info");
  check("버전 0.7 이상", /^0\.([7-9]|\d{2,})\./.test(info.version) || /^[1-9]\d*\./.test(info.version), info.version);
  const settings0 = await app.invoke("get_settings");
  check("주민등록번호 저장은 기본으로 꺼짐", settings0.store_rrn === false, String(settings0.store_rrn));

  // ── 1. 나이스 명렬표 가져오기 ──
  await app.nav("학생");
  await sleep(600);
  const tip = await app.evaluate(`window.__e2e.byText('나이스 명렬표 가져오기')?.title || ''`);
  check("학생 화면에 [나이스 명렬표 가져오기] + 받는 방법 안내", tip.includes("기본학적관리") && tip.includes("여러 학급 파일"), tip.slice(0, 60));
  await app.click("나이스 명렬표 가져오기");
  await waitFor(() => app.evaluate(`!!document.querySelector('.drop-zone')`), { label: "가져오기 창" });
  // 파일 끌어다 놓기: 창의 끌어다 놓기 이벤트를 그대로 보낸다
  const dropPaths = files(["학생명렬표.xlsx", "학생명렬표 1.xlsx", "학생명렬표 2.xlsx"]);
  await app.evaluate(
    `window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'tauri://drag-drop', payload: { paths: ${JSON.stringify(dropPaths)}, position: { x: 200, y: 200 } } }).then(() => true)`,
  );
  const table = await waitFor(async () => {
    const t = await app.text(".import-table").catch(() => "");
    return t.includes("학생명렬표 2.xlsx") ? t : null;
  }, { label: "끌어다 놓기 결과", timeout: 15000 }).catch(() => "");
  check("끌어다 놓은 파일 3개를 가져옴 (학급 1-1·2-1·3-1)", table.includes("1-1") && table.includes("2-1") && table.includes("3-1"), table.replace(/\s+/g, " ").slice(0, 160));
  await app.screenshot(path.join(shots, "60-neis-roster-import.png"));
  await app.click("닫기", ".dialog button");
  await sleep(300);
  // 같은 3-1 의 전체 열 파일 → 인원은 그대로, 정보만 보탬
  const r2 = await app.invoke("import_neis_roster", { paths: files(["학생명렬표 3.xlsx"]) });
  const f3 = r2.files[0];
  check("전체 열 파일 다시 가져오기: 인원 그대로·정보 보탬·주민번호는 건너뜀", f3 && f3.added === 0 && f3.updated === 2 && f3.rrn_skipped, JSON.stringify(f3));
  const r3 = await app.invoke("import_neis_roster", { paths: dropPaths });
  check("같은 파일 다시 가져오기 → 모두 ‘그대로’", r3.files.every((f) => f.added === 0 && f.updated === 0 && f.unchanged === f.total), r3.files.map((f) => `${f.file}:${f.unchanged}/${f.total}`).join(" "));
  const all = await app.invoke("list_students", { query: null, classId: null });
  check("학생 7명, 학급 3개", all.length === 7 && new Set(all.map((s) => s.class_label)).size === 3, all.map((s) => `${s.class_label}-${s.number}`).join(" "));
  const ordered = all.filter((s) => s.class_label === "1-1").map((s) => s.number);
  check("번호는 숫자 순 (1, 2, 10)", JSON.stringify(ordered) === "[1,2,10]", JSON.stringify(ordered));
  const bad = await app.invoke("import_neis_roster", { paths: [path.resolve(rosterDir, "없는파일.xlsx")] });
  check("열 수 없는 파일은 그 파일만 오류로 알림", bad.files[0]?.error, bad.files[0]?.error ?? "");

  // ── 2. 학생 상세: 명렬표 정보, 주소는 눌러야 보임 ──
  await app.nav("학생");
  await sleep(500);
  await app.click("남궁철수", ".student-card");
  await waitFor(() => app.evaluate(`!!document.querySelector('.student-info')`), { label: "학생 정보" });
  const before = await app.text(".student-info");
  check("학생 정보: 성별·생년월일·학생개인번호·이전학적 / 주소는 숨김", before.includes("2010년 1월 1일") && before.includes("9000000001") && before.includes("2학년1반1번") && !before.includes("OO로") && before.includes("눌러서 보기"), before.replace(/\n/g, " "));
  await app.click("눌러서 보기", ".student-info button");
  await sleep(200);
  const after = await app.text(".student-info");
  check("‘눌러서 보기’ → 주소(우편번호) 표시, 주민번호는 저장 안 됨", after.includes("(58000)") && after.includes("OO로 1") && !after.includes("주민등록번호"), after.replace(/\n/g, " ").slice(0, 120));
  await app.screenshot(path.join(shots, "61-student-info.png"));
  await app.click("학생 목록", "button");
  await sleep(300);
  // 학급명렬표의 '보일 항목'에 나이스 항목
  await app.click("학급명렬표", ".seg button");
  await sleep(500);
  const cols = await app.text(".roster-cols");
  check("학급명렬표 보일 항목에 학생개인번호·이전학적·비고", cols.includes("학생개인번호") && cols.includes("이전학적") && cols.includes("비고"), cols.replace(/\n/g, " "));
  await app.click("카드", ".seg button");

  // ── 3. 날짜 문장: 제목·시각·메모, 업무 기록 날짜 ──
  await app.invoke("create_duty", { input: { name: "교무부장", detail: "국제교류, 학사 운영" } });
  const md = `${tmr.getMonth() + 1}. ${tmr.getDate()}.`;
  const raw = `${md} 11:00 한일국제교류 컨설팅, OO고 홍길동 선생님 방문, 본관 2층 학생회의실`;
  const n1 = await app.invoke("create_note", { raw });
  const plan1 = await app.invoke("suggest_plan", { id: n1.id });
  const ev = plan1.items.filter((i) => i.kind === "event");
  check(
    `“${md} 11:00 …” → 제목·날짜·시각·메모`,
    ev.length === 1 && ev[0].text === "한일국제교류 컨설팅" && ev[0].date === tomorrow && ev[0].time === "11:00" && ev[0].memo === "OO고 홍길동 선생님 방문, 본관 2층 학생회의실",
    JSON.stringify(ev.map((e) => [e.text, e.date, e.time, e.memo])),
  );
  const duty = plan1.items.find((i) => i.kind === "duty");
  check("업무(교무부장) 분류 + 업무 기록 날짜도 그 날", duty && duty.date === tomorrow, JSON.stringify(duty ? [duty.duty_id, duty.date] : null));
  await app.invoke("save_plan", { id: n1.id, plan: plan1 });
  const week = await app.invoke("calendar", { view: "week", start: tomorrow, end: tomorrow });
  const evRow = week.find((i) => i.kind === "event" && i.title === "한일국제교류 컨설팅");
  check("일정표에 저장 (메모 포함)", evRow && evRow.time === "11:00" && (evRow.memo ?? "").includes("학생회의실"), JSON.stringify(evRow ?? null).slice(0, 160));
  const n1d = (await app.invoke("get_note_detail", { id: n1.id }).catch(() => null))?.note ?? (await app.invoke("list_notes", { filter: { limit: 5 } })).find((n) => n.id === n1.id);
  check("기록 카드 요약은 날짜를 뗀 제목", n1d && !/^\d/.test(n1d.summary ?? "") && (n1d.summary ?? "").startsWith("한일국제교류"), n1d?.summary ?? "");

  // ── 4. 학년·학급 인식 ──
  const n2 = await app.invoke("create_note", { raw: "1학년 심청, 전우치 수업중 잠 → 지도함" });
  const plan2 = await app.invoke("suggest_plan", { id: n2.id });
  const obs = plan2.items.filter((i) => i.kind === "student");
  check(
    "“1학년 심청, 전우치 …” → 두 학생 모두 1-1, 일정 아님",
    obs.length === 1 && obs[0].students.length === 2 && obs[0].students.every((s) => s.class_label === "1-1" && s.student_id) && !plan2.items.some((i) => i.kind === "event"),
    JSON.stringify(plan2.items.map((i) => [i.kind, i.students.map((s) => `${s.class_label} ${s.name}`)])),
  );
  for (const t of ["1학년1반 심청 발표 잘함", "1-1 심청 발표 잘함", "1년 심청 발표 잘함", "심청 전우치 지각"]) {
    const n = await app.invoke("create_note", { raw: t });
    const p = await app.invoke("suggest_plan", { id: n.id });
    const st = p.items.filter((i) => i.kind === "student").flatMap((i) => i.students);
    check(`“${t}” → 1-1`, st.length >= 1 && st.every((s) => s.class_label === "1-1" && s.student_id), st.map((s) => `${s.class_label} ${s.name}`).join(", "));
    await app.invoke("delete_note", { id: n.id });
  }
  const n3 = await app.invoke("create_note", { raw: "심청기 발표" });
  const p3 = await app.invoke("suggest_plan", { id: n3.id });
  check("두 글자 이름이 다른 낱말 속에 있으면 이름으로 보지 않음", !p3.items.some((i) => i.students.length), JSON.stringify(p3.items.map((i) => i.students)));
  await app.invoke("delete_note", { id: n3.id });
  await app.invoke("save_plan", { id: n2.id, plan: plan2 });

  // ── 5. 기록 세 탭과 기록 안 검색 ──
  const g = await app.invoke("create_note", { raw: "OO중 교무부장 연락옴" });
  await app.invoke("save_plan", { id: g.id, plan: await app.invoke("suggest_plan", { id: g.id }) });
  await app.nav("기록");
  await sleep(700);
  const seg = await app.text(".page-head .seg");
  check("기록 화면 탭: 모든 기록 / 기타 기록 / 정리된 기록", seg.includes("모든 기록") && seg.includes("기타 기록") && seg.includes("정리된 기록") && !seg.includes("미분류"), seg.replace(/\n/g, " "));
  const generalIds = async () => (await app.invoke("list_notes", { filter: { track: "general" } })).map((n) => n.id);
  const workIds = async () => (await app.invoke("list_notes", { filter: { track: "work" } })).map((n) => n.id);
  check("항목이 없는 기록은 기타 기록, 정리된 기록은 정리된 기록", (await generalIds()).includes(g.id) && (await workIds()).includes(n1.id) && (await workIds()).includes(n2.id), "");
  await app.screenshot(path.join(shots, "62-notes-general.png"));
  await app.evaluate(`(() => { const card = document.getElementById('note-${g.id}'); [...card.querySelectorAll('button')].find(b => b.innerText.includes('정리된 기록으로 보내기')).click(); return true; })()`);
  await sleep(700);
  check("[정리된 기록으로 보내기] → 정리된 기록 탭으로", (await workIds()).includes(g.id) && !(await generalIds()).includes(g.id), "");
  await app.click("정리된 기록", ".page-head .seg button");
  await sleep(700);
  const tag = await app.text(`#note-${g.id} .track-tag`);
  check("직접 옮긴 기록 표시", tag.includes("직접 옮김"), tag);
  await app.screenshot(path.join(shots, "63-notes-work.png"));
  await app.evaluate(`(() => { const card = document.getElementById('note-${g.id}'); [...card.querySelectorAll('button')].find(b => b.innerText.includes('기타 기록으로 되돌리기')).click(); return true; })()`);
  await sleep(700);
  check("[기타 기록으로 되돌리기] → 기타 기록 탭으로", (await generalIds()).includes(g.id), "");

  // ── 6. 오늘의 기록: 기타 기록은 흐리게 ──
  await app.nav("오늘");
  await sleep(800);
  const op = await app.evaluate(`(() => { const c = document.getElementById('note-${g.id}'); const w = document.getElementById('note-${n1.id}'); return { g: c ? getComputedStyle(c).opacity : null, w: w ? getComputedStyle(w).opacity : null }; })()`);
  check("오늘의 기록: 기타 기록은 흐리게, 정리된 기록은 또렷하게", Number(op.g) < 0.9 && Number(op.w) === 1, JSON.stringify(op));
  await app.evaluate(`document.getElementById('today-notes').scrollIntoView({ block: 'start' }); true`);
  await sleep(300);
  await app.screenshot(path.join(shots, "64-today-notes-dim.png"));

  // ── 7. 라이선스 및 저작권 ──
  await app.nav("설정");
  await sleep(800);
  const lic = await app.text("#set-license");
  check("설정 맨 아래 ‘라이선스 및 저작권’", lic.includes("차일영") && lic.includes("rraphop@me.com"), lic.replace(/\n/g, " ").slice(0, 100));
  const lastId = await app.evaluate(`[...document.querySelectorAll('.page.settings > section')].pop()?.id`);
  check("라이선스 메뉴가 설정 화면 맨 아래", lastId === "set-license", lastId);
  await app.click("라이선스 전문 보기", "#set-license button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.license-text')`), { label: "라이선스 창" });
  const full = await app.text(".license-text");
  check("라이선스 전문 (허용·금지·개인정보·책임의 한계)", ["[허용]", "[금지]", "[개인정보]", "[책임의 한계]", "THIRD_PARTY_NOTICES.txt"].every((k) => full.includes(k)), "");
  await app.screenshot(path.join(shots, "65-license.png"));
  await app.click("닫기", ".dialog button").catch(() => app.evaluate(`document.querySelector('.dialog .icon-btn, .dialog [aria-label="닫기"]')?.click(); true`));
  await sleep(300);
  await app.click("오픈소스·AI 모델 고지 보기", "#set-license button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.license-text')`), { label: "고지 창" });
  const notices = await app.text(".license-text");
  check("제3자 고지에 llama.cpp·Tauri·Qwen·calamine", ["llama.cpp", "Tauri", "Qwen", "calamine", "Apache"].every((k) => notices.includes(k)), "");

  // ── 업데이트 기록 ──
  await app.evaluate(`document.querySelector('.dialog [aria-label="닫기"]')?.click(); true`);
  await sleep(200);
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.changelog')`), { label: "업데이트 기록" });
  const log = await app.text(".dialog");
  check("업데이트 기록: v1.0 기능 소개 (나이스 명렬표·정리된 기록)", log.includes("v1.0") && log.includes("나이스 명렬표") && log.includes("정리된 기록"), "");
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  await fs.writeFile(path.join(shots, "results-v07.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
