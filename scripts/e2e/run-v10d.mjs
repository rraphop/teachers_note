// 1.0 3차 뒤 수정 E2E: 기록에 적은 학급 번호("번호는 21번") → 학생 명렬표 번호 칸 (학생부 자료로 보내지 않음),
//   기록 화면 세 탭(모든 기록 / 기타 기록 / 정리된 기록)과 기록 안에서 찾기(낱말 모두 · 구 그대로 · 찾은 곳 표시),
//   왼쪽 메뉴에서 여는 창(업데이트 기록)이 본문 위에 뜨고 휠이 창 안을 스크롤하는지.
// 학생·기관 이름은 모두 가짜(홍길동 등, OO군청)다.
// 실행: node scripts/e2e/run-v10d.mjs <teachers_note.exe> <데이터폴더> <스크린샷폴더>
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

const child = launch(exe, { dataRoot });
let app;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  for (const [c, n, name] of [
    ["3-1", null, "홍길동"],
    ["3-1", 20, "성춘향"],
  ])
    await app.invoke("create_student", { input: { name, class_label: c, number: n } });
  const studentsNow = async () => Object.fromEntries((await app.invoke("list_students", { query: null, classId: null })).map((s) => [s.name, s]));
  const classify = async (raw) => {
    const n = await app.invoke("create_note", { raw });
    return app.invoke("save_plan", { id: n.id, plan: await app.invoke("suggest_plan", { id: n.id }) });
  };

  // ── 1. 기록에 적은 학급 번호 → 명렬표 ──
  const n1 = await classify("3-1 홍길동 번호는 21번");
  let s = await studentsNow();
  check("‘번호는 21번’ → 명렬표 번호 21", s["홍길동"].number === 21, String(s["홍길동"].number));
  const d1 = await app.invoke("get_student_detail", { id: s["홍길동"].id });
  check("학생부 자료(관찰)로 보내지 않음", !d1.observations.some((o) => o.content.includes("번호")), JSON.stringify(d1.observations.map((o) => o.content)));
  check("기록 카드에 ‘학생 명렬표 · 번호’", n1.profile.some((p) => p.field === "number" && p.status === "applied"), JSON.stringify(n1.profile));
  check("명렬표 변경 이력에 남음", d1.profile_history.some((h) => h.field_label === "번호" && h.new_value === "21"), "");
  // 같은 학급에 그 번호인 학생이 있으면 바로 넣지 않고 기록 카드에서 확인
  const n2 = await classify("3-1 홍길동 번호를 20번으로 바꿈");
  s = await studentsNow();
  const pend = n2.profile.find((p) => p.field === "number");
  check("이미 번호가 있으면 확인 기다림 (21 → 20)", s["홍길동"].number === 21 && pend?.status === "pending" && pend.old_value === "21", JSON.stringify(pend));
  await app.invoke("resolve_profile_change", { id: pend.id, accept: false, replaceId: null });
  // 다른 번호(문항 번호 등)는 넣지 않는다
  const n3 = await classify("3-1 홍길동 수행평가 문항 번호 3번 틀림");
  check("‘문항 번호 3번’은 명렬표 번호가 아님", !n3.profile.some((p) => p.field === "number"), JSON.stringify(n3.profile));

  // ── 2. 기록 화면: 세 탭 ──
  await classify("OO군청에 행사 일정 물어봄");
  await classify("다음 주 수요일 15시 학년 협의회");
  await classify("홍길동 보호자와 진로 상담함. 보호자 연락은 다음 주");
  await app.nav("기록");
  await waitFor(() => app.evaluate(`!!document.querySelector('.notes-search input')`), { label: "기록 화면" });
  await sleep(600);
  const seg = await app.text(".page-head .seg");
  check("탭: 모든 기록 / 기타 기록 / 정리된 기록", ["모든 기록", "기타 기록", "정리된 기록"].every((t) => seg.includes(t)) && !seg.includes("일반 기록"), seg.replace(/\n/g, " "));
  const on = await app.text(".page-head .seg button.on");
  check("처음엔 ‘모든 기록’", on.includes("모든 기록"), on);
  const cards = async () => app.evaluate(`document.querySelectorAll('.note-list .note-card').length`);
  const all = await app.invoke("list_notes", { filter: { limit: 500 } });
  check("모든 기록 = 기타 + 정리된 기록 전부", (await cards()) === all.length, `${await cards()} / ${all.length}`);
  const tags = await app.evaluate(`[...document.querySelectorAll('.note-list .track-tag')].map(t => t.innerText.trim())`);
  check("카드 표시: 기타 기록 / 정리된 기록", tags.some((t) => t.startsWith("기타 기록")) && tags.some((t) => t.startsWith("정리된 기록")) && !tags.some((t) => t.includes("업무기록") || t.includes("일반")), tags.join(","));
  await app.screenshot(path.join(shots, "01-notes-all.png"));

  // ── 3. 기록 안에서 찾기 ──
  const search = async (q) => {
    await app.evaluate(`window.__e2e.setValue(document.querySelector('.notes-search input'), ${JSON.stringify(q)})`);
    await sleep(900);
    return app.evaluate(`[...document.querySelectorAll('.note-list .note-card .note-raw')].map(e => e.innerText)`);
  };
  let got = await search("홍길동 상담");
  check("낱말 두 개 → 둘 다 든 기록만 (붙어 있지 않아도)", got.length === 1 && got[0].includes("진로 상담"), JSON.stringify(got));
  const marks = await app.evaluate(`[...document.querySelectorAll('.note-list mark.hit')].map(m => m.innerText)`);
  check("찾은 곳 표시", marks.includes("홍길동") && marks.includes("상담"), marks.join(","));
  got = await search("홍길동 협의회");
  const empty = await app.evaluate(`document.querySelector('.empty')?.innerText ?? ''`);
  check("둘 중 하나만 든 기록은 나오지 않음", got.length === 0 && empty.includes("찾는 말"), empty.replace(/\n/g, " "));
  got = await search("보호자 연락");
  check("낱말 모두: ‘보호자’와 ‘연락’이 든 기록", got.length === 1, JSON.stringify(got));
  got = await search("행사 물어봄");
  check("낱말 모두: 떨어져 있어도 찾음", got.length === 1 && got[0].includes("OO군청"), JSON.stringify(got));
  await app.click("구 그대로", ".notes-search .seg button");
  await sleep(900);
  got = await app.evaluate(`[...document.querySelectorAll('.note-list .note-card .note-raw')].map(e => e.innerText)`);
  check("구 그대로: ‘행사 물어봄’은 붙어 있지 않아 없음", got.length === 0, JSON.stringify(got));
  got = await search("행사 일정");
  check("구 그대로: ‘행사 일정’ 그대로 든 기록", got.length === 1, JSON.stringify(got));
  await app.click("낱말 모두", ".notes-search .seg button");
  got = await search('"진로 상담" 보호자');
  check("따옴표로 묶은 구 + 낱말", got.length === 1 && got[0].includes("진로 상담"), JSON.stringify(got));
  await app.screenshot(path.join(shots, "02-notes-search.png"));
  const counts = await app.text(".page-head .seg");
  check("찾는 중에는 탭 숫자도 찾은 수", /모든 기록\s*1/.test(counts), counts.replace(/\n/g, " "));
  await app.click("기타 기록", ".page-head .seg button");
  await sleep(700);
  check("찾는 중에 탭을 바꾸면 그 탭 안에서 찾기", (await cards()) === 0, String(await cards()));
  await app.click("지우기", ".notes-search button");
  await sleep(900);
  const generalCount = (await app.invoke("list_notes", { filter: { track: "general", limit: 500 } })).length;
  check("지우기 → 기타 기록 전부", (await cards()) === generalCount, `${await cards()} / ${generalCount}`);

  // ── 4. 업데이트 기록 창 (왼쪽 메뉴에서 여는 창): 본문 위에 뜨고, 휠은 창 안을 스크롤 ──
  await app.nav("오늘");
  await app.send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 640, deviceScaleFactor: 1, mobile: false });
  await sleep(600);
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.dialog .changelog')`), { label: "업데이트 기록" });
  await sleep(300);
  const box = await app.evaluate(`(() => {
    const d = document.querySelector('.dialog'); const r = d.getBoundingClientRect();
    const pts = [[r.left + r.width * 0.8, r.top + r.height * 0.5], [r.left + r.width * 0.5, r.bottom - 12], [r.left + 30, r.top + 70]];
    return { underBody: d.closest('.overlay').parentElement === document.body, onTop: pts.every(([x, y]) => d.contains(document.elementFromPoint(x, y))),
      inView: r.bottom <= window.innerHeight, locked: document.documentElement.classList.contains('modal-open'),
      x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) };
  })()`);
  check("업데이트 기록 창이 본문(아래 입력창·급식 칸) 위에 뜸", box.underBody && box.onTop && box.inView, JSON.stringify(box));
  const before = await app.evaluate(`({ body: document.querySelector('.dialog-body').scrollTop, page: window.scrollY, can: document.querySelector('.dialog-body').scrollHeight > document.querySelector('.dialog-body').clientHeight })`);
  await app.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: box.x, y: box.y, deltaX: 0, deltaY: 400 });
  await sleep(600);
  const after = await app.evaluate(`({ body: document.querySelector('.dialog-body').scrollTop, page: window.scrollY })`);
  check("휠 → 업데이트 내용이 스크롤되고 뒤 화면은 그대로", before.can && after.body > before.body && after.page === before.page && box.locked, `${JSON.stringify(before)} → ${JSON.stringify(after)}`);
  await app.screenshot(path.join(shots, "03-update-log-small.png"));
  await app.evaluate(`window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))`);
  await sleep(300);
  const unlocked = await app.evaluate(`!document.querySelector('.dialog') && !document.documentElement.classList.contains('modal-open')`);
  check("Esc 로 닫으면 뒤 화면 스크롤 다시 됨", unlocked, "");
  await app.send("Emulation.clearDeviceMetricsOverride", {});
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  await fs.writeFile(path.join(shots, "results-v10d.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
