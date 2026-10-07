// 0.5 E2E: 입력창의 시간표 문장 → 개인 시간표, 충돌 확인(기록 카드·시간표 화면), 하루짜리 변경(오늘 시간표·일정표),
//          교시 → 시각, 오늘의 기록(이름·날짜 찾기)
// 실행: node scripts/e2e/run-v05.mjs <exe> <데이터폴더> <스크린샷폴더>
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
const iso = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const now = new Date();
const today = iso(now);
const tomorrow = iso(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1));

const child = launch(exe, { dataRoot });
let app;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  const info = await app.invoke("app_info");
  check("버전 0.5 이상", /^0\.([5-9]|\d{2,})\./.test(info.version) || /^[1-9]\d*\./.test(info.version), info.version);
  const sem = await app.invoke("current_semester");
  const personal = () => app.invoke("get_timetable", { year: sem.school_year, semester: sem.semester, kind: "personal", classLabel: null });
  const write = async (text) => {
    await app.type(".capture textarea", text);
    await app.click("기록", ".capture button");
    await sleep(900);
  };
  const lastNote = async () => (await app.invoke("list_notes", { filter: { status: "all", limit: 1 } }))[0];

  // ── 1. 정규 시간표 문장 → 개인 시간표 ──
  await write("월 3교시 2-1 국어 과학실\n화·목 1교시 3-2 수학\n매주 수요일 5·6교시 동아리");
  let n1 = await lastNote();
  let v = await personal();
  const cell = (wd, p) => v.cells.find((c) => c.weekday === wd && c.period === p);
  check(
    "입력창 시간표 문장 → 개인 시간표 5칸",
    v.cells.length === 5 && cell(1, 3)?.class_label === "2-1" && cell(1, 3)?.room === "과학실" && cell(2, 1)?.subject === "수학" && cell(4, 1)?.class_label === "3-2" && cell(3, 6)?.subject === "동아리",
    v.cells.map((c) => `${c.weekday}-${c.period}:${c.class_label} ${c.subject} ${c.room}`).join(" / "),
  );
  check("시간표 문장만 있는 기록은 AI 없이 바로 정리", n1.status === "classified" && n1.classified_by === "rule" && n1.chips.filter((c) => c.kind === "timetable").length === 5, `${n1.status}/${n1.classified_by} 칩 ${n1.chips.length}`);
  check("시간표 문장은 학생 관찰·일정·할 일로 만들지 않음", n1.chips.every((c) => c.kind === "timetable"), n1.chips.map((c) => c.kind).join(","));

  // ── 2. 다른 수업이 있는 칸: 기록 카드에서 '바꾸기' ──
  await write("월 3교시 2-3 국어");
  const n2 = await lastNote();
  v = await personal();
  check("다른 수업이 있는 칸은 바로 덮어쓰지 않음", cell(1, 3)?.class_label === "2-1" && n2.chips.some((c) => c.kind === "tt_conflict"), n2.chips.map((c) => `${c.kind}:${c.label}`).join(" / "));
  await sleep(300);
  await app.screenshot(path.join(shots, "53-tt-conflict-chip.png"));
  await app.evaluate(`(() => { const chip = [...document.querySelectorAll('.chip.tt_conflict')][0]; [...chip.querySelectorAll('button')].find(b => b.innerText.includes('바꾸기')).click(); return true; })()`);
  await sleep(800);
  v = await personal();
  check("기록 카드의 ‘바꾸기’ → 칸이 바뀜 (장소는 새 문장대로)", cell(1, 3)?.class_label === "2-3" && cell(1, 3)?.room === "", JSON.stringify(cell(1, 3)));

  // ── 3. 시간표 화면의 확인 목록에서 '그대로' ──
  await write("화 1교시 1-1 영어");
  await app.nav("시간표");
  await sleep(900);
  const banner = await app.text(".tt-conflicts").catch(() => "");
  check("시간표 화면에 ‘기록에서 온 시간표 확인 1건’", banner.includes("확인 1건") && banner.includes("화 1교시 1-1 영어") && banner.includes("3-2 수학"), banner.replace(/\n/g, " ").slice(0, 120));
  await app.screenshot(path.join(shots, "54-tt-conflict-banner.png"));
  await app.click("그대로", ".tt-conflicts button");
  await sleep(800);
  v = await personal();
  const conflictsLeft = await app.invoke("tt_conflicts");
  check("‘그대로’ → 원래 수업 유지, 확인 목록 비움", cell(2, 1)?.class_label === "3-2" && conflictsLeft.length === 0, JSON.stringify(cell(2, 1)));
  await app.screenshot(path.join(shots, "55-timetable-from-notes.png"));

  // ── 4. 하루짜리 변경 ──
  await app.nav("오늘");
  await sleep(600);
  await write("오늘 2교시 1-3 보강\n내일 3교시 2-1 교체");
  const n4 = await lastNote();
  check("하루짜리 변경 2건 (칸은 그대로)", n4.chips.filter((c) => c.kind === "tt_change").length === 2 && (await personal()).cells.length === 5, n4.chips.map((c) => c.label).join(" / "));
  const tt = await app.invoke("today_timetable");
  const slot2 = tt.slots.find((s) => s.period.no === 2);
  check("오늘 시간표 2교시에 ‘보강 1-3’ 표시", slot2?.change?.change_type === "보강" && slot2?.change?.class_label === "1-3", JSON.stringify(slot2?.change ?? null).slice(0, 120));
  const wk = await app.invoke("calendar", { view: "week", start: today, end: tomorrow });
  const ch = wk.filter((i) => i.kind === "tt_change");
  check("주간 일정표에 ‘시간표 변경’ 2건 (교시 시각 포함)", ch.length === 2 && ch.every((i) => i.time) && ch.some((i) => i.start_date === tomorrow && i.title.includes("교체")), ch.map((i) => `${i.start_date} ${i.time} ${i.title}`).join(" / "));
  const toastText = await app.text(".toasts");
  check("시간표만 적은 기록의 알림은 ‘시간표에 반영’", toastText.includes("시간표에 반영했습니다") && !toastText.split("\n").slice(-1)[0].includes("미분류"), toastText.replace(/\n/g, " / ").slice(-120));
  await app.evaluate(`document.querySelector('.today-tt').scrollIntoView({ block: 'start' }); true`);
  await sleep(300);
  await app.screenshot(path.join(shots, "56-today-tt-change.png"));

  // ── 5. 교시 → 시각 (직접 분류로 저장) ──
  const n5 = await app.invoke("create_note", { raw: "오늘 5교시 전교생 소방 대피 훈련" });
  const plan = await app.invoke("suggest_plan", { id: n5.id });
  // 교사가 직접 분류 창에서 일정으로 저장하는 경우 (규칙 제안에 없으면 교사가 추가한 것처럼)
  if (!plan.items.some((i) => i.kind === "event")) {
    plan.items.push({ kind: "event", text: "5교시 전교생 소방 대피 훈련", students: [], area: "none", duty_id: null, date: today, time: null, date_expr: "오늘", approximate: false, remind: null, origin: "manual", done: false, edited: false });
  }
  await app.invoke("save_plan", { id: n5.id, plan });
  const evs = (await app.invoke("calendar", { view: "week", start: today, end: today })).filter((i) => i.kind === "event" && i.title.includes("소방"));
  const bell = await app.invoke("get_bell", { year: sem.school_year, semester: sem.semester });
  const p5 = bell.periods.find((p) => p.no === 5);
  check("‘5교시’ 일정에 5교시 시작 시각 자동 입력", evs.length === 1 && evs[0].time === p5?.start, `${evs[0]?.time} (5교시 ${p5?.start})`);
  const n6 = await app.invoke("create_note", { raw: "오늘 3교시 2-1 홍길동 지각" });
  const n6d = (await app.invoke("list_notes", { filter: { status: "all", limit: 1 } }))[0];
  check("학생·출결 문장은 시간표로 보지 않음", n6.id === n6d.id && !n6d.chips.some((c) => ["timetable", "tt_conflict", "tt_change"].includes(c.kind)) && (await personal()).cells.length === 5, n6d.chips.map((c) => c.kind).join(","));

  // ── 6. 오늘의 기록 (이름·날짜 찾기) ──
  await app.nav("오늘");
  await sleep(700);
  const head = await app.text("#today-notes .section-head");
  const pos = await app.evaluate(`(() => { const s = document.querySelector('#today-notes').getBoundingClientRect(); const f = document.querySelector('#today-notes .date-finder').getBoundingClientRect(); return { gap: Math.round(s.right - f.right), top: Math.round(f.top - s.top) }; })()`);
  check("‘오늘의 기록’ 이름 + 오른쪽 끝 날짜 찾기", head.includes("오늘의 기록") && !head.includes("오늘 쓴 기록") && pos.gap < 40 && pos.top < 60, JSON.stringify(pos));
  const countToday = (await app.invoke("list_notes", { filter: { status: "all", date: today } })).length;
  await app.screenshot(path.join(shots, "57-today-notes-date.png"));
  await app.evaluate(`document.querySelector('#today-notes .date-finder button[aria-label="하루 전"]').click()`);
  await sleep(700);
  const headY = await app.text("#today-notes .section-head");
  const emptyY = await app.text("#today-notes");
  check("‹ 하루 전 → 어제 기록 (없으면 안내)", !headY.includes("오늘의 기록") && headY.includes("의 기록") && emptyY.includes("쓴 기록이 없습니다"), headY.replace(/\n/g, " "));
  await app.evaluate(`[...document.querySelectorAll('#today-notes .date-finder button')].find(b => b.innerText.trim() === '오늘').click()`);
  await sleep(700);
  const cards = await app.evaluate(`document.querySelectorAll('#today-notes .note-card').length`);
  check("‘오늘’ 버튼 → 오늘의 기록으로 돌아옴", cards === countToday, `${cards}/${countToday}`);

  // ── 업데이트 기록 ──
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.changelog')`), { label: "업데이트 기록" });
  const log = await app.text(".dialog");
  check("업데이트 기록: v1.0 기능 소개 (시간표·오늘의 기록)", log.includes("v1.0") && log.includes("시간표") && log.includes("오늘의 기록"), "");
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  await fs.writeFile(path.join(shots, "results-v05.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
