// 1.0 3차 수정목록 E2E: 학생 명렬표 표 폭(비율로 화면 채우기)·학생 추가 줄(학급·다음 번호), 기록 줄바꿈 표시,
//   끝난 일은 할 일로 만들지 않기, 할 일 완료 → 완료 기록(일반 기록·업무 기록)과 되돌리기, 창 닫기 → 트레이·빠른 기록 창·종료,
//   (gguf 를 주면) AI 계산 장치: 그래픽카드가 있으면 GPU, CPU 만 설정과 속도 비교.
// 학생·학교·기관 이름은 모두 가짜(홍길동 등, OO군청)다.
// 실행: node scripts/e2e/run-v10c.mjs <teachers_note.exe> <데이터폴더> <스크린샷폴더> [gguf 경로]
import { execFileSync } from "node:child_process";
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
const near = (a, b, tol = 2) => Math.abs(a - b) <= tol;
const view = async (app, width, height = 1000) => {
  await app.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await sleep(500);
};
const ps = (script) => execFileSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).trim();
const alive = (pid) => ps(`if (Get-Process -Id ${pid} -ErrorAction SilentlyContinue) { 'yes' } else { 'no' }`) === "yes";

const child = launch(exe, { dataRoot });
let app;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });

  // ── 준비: 가짜 학생 (1-1 셋, 1-2 둘, 2-1 둘), 담임반 1-2 ──
  for (const [c, n, name] of [
    ["1-1", 1, "홍길동"],
    ["1-1", 2, "성춘향"],
    ["1-1", 3, "이몽룡"],
    ["1-2", 1, "임꺽정"],
    ["1-2", 2, "전우치"],
    ["2-1", 1, "심청"],
    ["2-1", 2, "흥부"],
  ])
    await app.invoke("create_student", { input: { name, class_label: c, number: n } });
  const st0 = await app.invoke("get_settings");
  await app.invoke("update_settings", { value: { ...st0, homeroom: "1-2" } });

  // ── 1. 학생 명렬표: 표는 늘 본문 폭을 채우고 열은 비율대로 ──
  await view(app, 1920);
  await app.nav("학생 명렬표");
  await waitFor(() => app.evaluate(`!!document.querySelector('.student-search')`), { label: "학생 화면" });
  await app.click("학급명렬표", ".seg button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.roster-table') && !!document.querySelector('.roster-cols input:not([disabled])')`), { label: "학급명렬표" });
  const setCols = async (labels) => {
    await app.evaluate(`(() => {
      const want = ${JSON.stringify(labels)};
      for (const l of document.querySelectorAll('.roster-cols label.check')) {
        const box = l.querySelector('input');
        if (box.checked !== want.includes(l.innerText.trim())) box.click();
      }
      return true;
    })()`);
    await waitFor(() => app.evaluate(`[...document.querySelectorAll('.roster-table thead th')].filter(th => getComputedStyle(th).display !== 'none').length === ${labels.length + 4}`), { label: "보일 항목", timeout: 6000 });
    await sleep(300);
  };
  const measure = () =>
    app.evaluate(`(() => {
      const box = document.querySelector('.roster-scroll');
      const heads = [...document.querySelectorAll('.roster-table thead th')].filter(th => getComputedStyle(th).display !== 'none').map(th => {
        const b = th.getBoundingClientRect();
        return { t: th.innerText.trim(), l: Math.round(b.left), r: Math.round(b.right), w: Math.round(b.width) };
      });
      const add = document.querySelector('.roster-table tr.add-row').getBoundingClientRect();
      return { heads, boxW: box.clientWidth, scrollW: box.scrollWidth, tableW: Math.round(document.querySelector('.roster-table').getBoundingClientRect().width), addW: Math.round(add.width) };
    })()`);
  const col = (m, t) => m.heads.find((h) => h.t.startsWith(t));
  await setCols([]);
  let m = await measure();
  check(
    "선택 항목 0개: 네 칸이 표 폭을 다 채움 (비율 학급 1 · 번호 0.7 · 이름 1.5 · 기록 1)",
    near(m.tableW, m.boxW, 3) && m.scrollW <= m.boxW && near(col(m, "이름").w / col(m, "학급").w, 1.5, 0.05) && near(col(m, "번호").w / col(m, "학급").w, 0.7, 0.05) && m.heads[3].t === "기록",
    JSON.stringify(m.heads.map((h) => [h.t, h.w])) + ` 표 ${m.tableW}/${m.boxW}`,
  );
  check("  학생 추가 줄도 표 폭 전체", near(m.addW, m.tableW, 3), `${m.addW}/${m.tableW}`);
  await setCols(["전화번호"]);
  m = await measure();
  check("전화번호만 켬: 다섯 칸이 표 폭을 다 채움, 기록은 맨 오른쪽", m.heads.length === 5 && near(m.tableW, m.boxW, 3) && m.heads[4].t === "기록" && near(col(m, "전화번호").w / col(m, "학급").w, 1.5, 0.05), JSON.stringify(m.heads.map((h) => [h.t, h.w])));
  await view(app, 1280);
  m = await measure();
  check("  창을 줄여도(1280px) 표 폭을 다 채움", near(m.tableW, m.boxW, 3) && m.scrollW <= m.boxW, `${m.tableW}/${m.boxW}`);
  await setCols(["전화번호", "보호자 연락처", "주소", "성별", "생일", "특이사항", "학생개인번호", "이전학적", "비고"]);
  m = await measure();
  const pinned = () =>
    app.evaluate(`(() => {
      const box = document.querySelector('.roster-scroll');
      const tr = document.querySelector('.roster-table tbody tr:not(.add-row)');
      const bl = box.getBoundingClientRect().left;
      return [...tr.children].slice(0, 4).map(td => Math.round(td.getBoundingClientRect().left - bl));
    })()`);
  const before = await pinned();
  await app.evaluate(`document.querySelector('.roster-scroll').scrollLeft = 400, true`);
  await sleep(300);
  const after = await pinned();
  check("전체 켬(1280px): 최소 폭 합이 넘쳐 가로 스크롤, 학급·번호·이름은 왼쪽 고정", m.scrollW > m.boxW && before.slice(0, 3).every((x, i) => near(x, after[i])) && after[3] < before[3] - 100, `${m.scrollW}/${m.boxW} ${JSON.stringify(before)}→${JSON.stringify(after)}`);
  await app.evaluate(`document.querySelector('.roster-scroll').scrollLeft = 0, true`);
  await setCols([]);
  await view(app, 1920);
  await app.screenshot(path.join(shots, "c1-roster-fill.png"));

  // ── 3. 학생 추가 줄: 학급은 고른 학급(전체면 담임반), 번호는 마지막 번호 + 1 ──
  const addRow = () =>
    app.evaluate(`({
      cls: document.querySelector('.add-row select[aria-label="추가할 학생의 학급"]')?.value ?? document.querySelector('.add-row input[aria-label="새 학급"]')?.value,
      num: document.querySelector('.add-row input[aria-label="추가할 학생의 번호"]').value,
    })`);
  const pickClassBtn = async (label) => {
    await app.evaluate(`[...document.querySelectorAll('.class-filter button')].find(b => b.innerText.trim().split(/\\s/)[0] === ${JSON.stringify(label)}).click(), true`);
    await sleep(500);
  };
  await pickClassBtn("2-1");
  let a = await addRow();
  check("2-1 선택 → 학급 2-1, 번호 3 (실제 값)", a.cls === "2-1" && a.num === "3", JSON.stringify(a));
  await pickClassBtn("1-1");
  a = await addRow();
  check("1-1 선택 → 학급 1-1, 번호 4", a.cls === "1-1" && a.num === "4", JSON.stringify(a));
  await pickClassBtn("전체");
  a = await addRow();
  check("전체 학급 → 담임반 1-2, 번호 3", a.cls === "1-2" && a.num === "3", JSON.stringify(a));
  // 학급 드롭다운을 바꾸면 번호도 그 학급 기준
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.add-row select[aria-label="추가할 학생의 학급"]'), '1-1')`);
  await sleep(200);
  a = await addRow();
  check("  드롭다운에서 1-1 로 바꾸면 번호 4", a.cls === "1-1" && a.num === "4", JSON.stringify(a));
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.add-row select[aria-label="추가할 학생의 학급"]'), '1-2')`);
  await sleep(200);
  // 이름만 적고 Enter → 추가, 번호 +1, 커서는 이름 칸
  await app.type('.add-row input[aria-label="추가할 학생의 이름"]', "장보고");
  await app.evaluate(`document.querySelector('.add-row input[aria-label="추가할 학생의 이름"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
  await waitFor(async () => ((await app.invoke("list_students", { query: null, classId: null })).some((s) => s.name === "장보고") ? true : null), { label: "학생 추가", timeout: 6000 });
  const jang = (await app.invoke("list_students", { query: null, classId: null })).find((s) => s.name === "장보고");
  await sleep(500);
  a = await addRow();
  const focusName = await app.evaluate(`document.activeElement?.getAttribute('aria-label')`);
  check("이름 입력 후 Enter → 1-2 3번으로 추가, 번호 4로, 커서는 이름 칸", jang?.class_label === "1-2" && jang?.number === 3 && a.num === "4" && focusName === "추가할 학생의 이름", `${jang?.class_label} ${jang?.number} · 다음 ${a.num} · 커서 ${focusName}`);
  // 같은 학급 같은 번호면 추가 전에 알림
  await app.type('.add-row input[aria-label="추가할 학생의 번호"]', "1");
  await app.type('.add-row input[aria-label="추가할 학생의 이름"]', "김유신");
  await app.evaluate(`document.querySelector('.add-row input[aria-label="추가할 학생의 이름"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
  const dlg = await waitFor(() => app.evaluate(`document.querySelector('.dialog')?.innerText || null`), { label: "겹침 알림", timeout: 5000 }).catch(() => null);
  check("같은 학급·번호가 있으면 추가 전에 알림", !!dlg && dlg.includes("임꺽정") && dlg.includes("1-2 1번"), (dlg ?? "").replace(/\s+/g, " ").slice(0, 80));
  await app.evaluate(`[...document.querySelectorAll('.dialog button')].find(b => b.innerText.trim() === '취소')?.click(), true`);
  await sleep(300);
  check("  [취소] → 추가하지 않음", !(await app.invoke("list_students", { query: null, classId: null })).some((s) => s.name === "김유신"), "");
  await app.click("카드", ".seg button");

  // ── 4. 기록 줄바꿈 표시 ──
  const n1 = await app.invoke("create_note", { raw: "10. 6. 오전 9:30 경 OO교육지원청 학교지원센터에 강사수당 출처 물어봄, 담당자 홍길동. 10.6. 오전 9:40 경 OO군청 문화체육관광과에 참석 인원 물어봄" });
  const n2 = await app.invoke("create_note", { raw: "첫째 줄 메모\n둘째 줄 메모" });
  const n3 = await app.invoke("create_note", { raw: "09:36 1교시 시작 09:46 지각생 확인, 10:02 보건실 이동" });
  await app.nav("기록");
  await waitFor(() => app.evaluate(`!!document.querySelector('#note-${n3.id}')`), { label: "기록 화면", timeout: 8000 });
  const lines = (id) => app.evaluate(`[...document.querySelectorAll('#note-${id} .note-raw .tline')].map(l => ({ lead: l.querySelector('.tline-lead')?.innerText ?? '', text: l.innerText }))`);
  const l1 = await lines(n1.id);
  check("한 문단에 붙은 두 건 → 두 줄, 줄 앞 시각은 굵게", l1.length === 2 && l1[0].lead === "10. 6. 오전 9:30" && l1[1].lead === "10.6. 오전 9:40", JSON.stringify(l1.map((l) => l.lead)));
  const l2 = await lines(n2.id);
  check("Enter 로 나눈 기록은 적은 그대로 두 줄", l2.length === 2 && l2[1].text === "둘째 줄 메모", JSON.stringify(l2.map((l) => l.text)));
  const l3 = await lines(n3.id);
  check("시각마다 줄바꿈 (09:36 · 09:46 · 10:02)", l3.length === 3 && l3.map((l) => l.lead).join(",") === "09:36,09:46,10:02", JSON.stringify(l3.map((l) => l.lead)));
  const rawKept = (await app.invoke("get_note_detail", { id: n1.id })).note.raw_text;
  check("  원문은 그대로 (화면 표시만 바뀜)", rawKept.includes("담당자 홍길동. 10.6. 오전 9:40") && !rawKept.includes("\n"), "");
  await app.screenshot(path.join(shots, "c2-note-lines.png"));
  // 교무 업무의 업무 기록에도
  const dutyId = await app.invoke("create_duty", { input: { name: "교무부장", detail: "국제교류", guide: "", keywords: "", active: true } });
  await app.invoke("create_duty_log", { input: { duty_id: dutyId, content: "10. 6. 오전 9:30 경 OO교육지원청에 강사수당 출처 물어봄. 10.6. 오전 9:40 경 OO군청에 참석 인원 물어봄", date: null, time: null } });
  await app.nav("교무 업무");
  await waitFor(() => app.evaluate(`!!document.querySelector('.log-content .tline')`), { label: "업무 기록 줄", timeout: 8000 }).catch(() => null);
  const dl = await app.evaluate(`[...document.querySelectorAll('.log-content')].map(p => p.querySelectorAll('.tline').length)`);
  check("교무 업무의 업무 기록도 두 줄로", dl.includes(2), JSON.stringify(dl));

  // ── 5. 이미 끝난 일은 할 일로 만들지 않기 (AI 없이 규칙 제안을 저장하는 흐름) ──
  const ruleNote = async (raw) => {
    const n = await app.invoke("create_note", { raw });
    const plan = await app.invoke("suggest_plan", { id: n.id });
    await app.invoke("save_plan", { id: n.id, plan });
    return plan;
  };
  const tasksOf = (plan) => plan.items.filter((i) => i.kind === "task");
  const friday = (() => {
    const d = new Date();
    const add = (5 - d.getDay() + 7) % 7;
    return iso(new Date(d.getFullYear(), d.getMonth(), d.getDate() + add));
  })();
  let pl = await ruleNote("10.6. 오전 9:40 OO군청에 참석 인원 물어봄");
  check("‘10.6. 오전 9:40 군청에 참석 인원 물어봄’ → 할 일 없음", tasksOf(pl).length === 0, JSON.stringify(pl.items.map((i) => i.kind)));
  pl = await ruleNote("OO군청에 참석 인원 물어볼 것");
  check("‘군청에 참석 인원 물어볼 것’ → 할 일, 마감 없음", tasksOf(pl).length === 1 && tasksOf(pl)[0].date === null, JSON.stringify(tasksOf(pl).map((t) => [t.text, t.date])));
  pl = await ruleNote("금요일까지 OO군청에 참석 인원 회신");
  check("‘금요일까지 군청에 참석 인원 회신’ → 할 일, 마감 금요일", tasksOf(pl).length === 1 && tasksOf(pl)[0].date === friday, JSON.stringify(tasksOf(pl).map((t) => [t.text, t.date])));
  pl = await ruleNote("OO군청에 물어봄. 답 오면 금요일까지 교장 보고");
  check("‘군청에 물어봄. 답 오면 금요일까지 교장 보고’ → 할 일은 ‘교장 보고’만", tasksOf(pl).length === 1 && tasksOf(pl)[0].text === "교장 보고" && tasksOf(pl)[0].date === friday, JSON.stringify(tasksOf(pl).map((t) => [t.text, t.date])));

  // ── 6. 할 일 완료 → 완료 기록, 되돌리면 지움 ──
  await app.invoke("create_task", { input: { title: "OO교육청 공문 회신", due_date: null } });
  await app.invoke("create_task", { input: { title: "교류 학교 일정표 정리", due_date: null, duty_id: dutyId } });
  await app.nav("할 일");
  await waitFor(() => app.evaluate(`!!document.querySelector('.agenda-row input[aria-label="교류 학교 일정표 정리 완료"]')`), { label: "할 일", timeout: 8000 });
  await app.evaluate(`document.querySelector('.agenda-row input[aria-label="OO교육청 공문 회신 완료"]').click(), true`);
  await app.evaluate(`document.querySelector('.agenda-row input[aria-label="교류 학교 일정표 정리 완료"]').click(), true`);
  await sleep(1200);
  const notes = await app.invoke("list_notes", { filter: { limit: 100 } });
  const doneNote = notes.find((n) => n.from_task_id !== null && n.raw_text.endsWith("OO교육청 공문 회신"));
  check(
    "업무 없는 할 일 완료 → 일반 기록 ‘M월 D일 HH:MM 완료 · 제목’",
    !!doneNote && /^\d{1,2}월 \d{1,2}일 \d{2}:\d{2} 완료 · OO교육청 공문 회신$/.test(doneNote.raw_text) && !doneNote.is_work && doneNote.status === "classified",
    doneNote?.raw_text,
  );
  const logs = await app.invoke("list_duty_logs", { dutyId: dutyId }).catch(() => null);
  const board = logs ?? (await app.invoke("duty_board", { dutyId: dutyId }).catch(() => null));
  const allLogs = Array.isArray(board) ? board : board?.logs ?? [];
  const doneLog = allLogs.find((l) => l.from_task_id !== null);
  check("업무와 연결된 할 일 완료 → 교무 업무의 업무 기록 (직접, 새 항목 숫자 아님)", !!doneLog && /완료 · 교류 학교 일정표 정리$/.test(doneLog.content) && doneLog.source === "manual", doneLog?.content);
  const ov = await app.invoke("today_overview");
  check("  새 항목 숫자에 넣지 않음", ov.new_counts.records === 0 && ov.new_counts.duties === 0 && ov.new_counts.tasks === 0, JSON.stringify(ov.new_counts));
  await app.click("완료", ".seg button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.agenda-row .done-record')`), { label: "완료 탭", timeout: 6000 }).catch(() => null);
  const doneTags = await app.evaluate(`[...document.querySelectorAll('.agenda-row .done-record')].map(t => t.innerText)`);
  check("할 일 [완료] 탭에 완료 시각 표시", doneTags.length === 2 && doneTags.every((t) => t.endsWith("완료")), JSON.stringify(doneTags));
  await app.screenshot(path.join(shots, "c3-tasks-done.png"));
  // 기록 화면의 완료 기록 카드
  await app.nav("기록");
  await waitFor(() => app.evaluate(`!!document.querySelector('#note-${doneNote.id}')`), { label: "완료 기록 카드", timeout: 8000 }).catch(() => null);
  check("기록 화면: 완료 기록 카드에 ‘할 일 완료’ 표시", await app.evaluate(`(document.querySelector('#note-${doneNote.id} .done-record')?.innerText || '') === '할 일 완료'`), "");
  // 다시 분류해도 할 일로 바뀌지 않음
  await app.invoke("save_plan", { id: doneNote.id, plan: await app.invoke("suggest_plan", { id: doneNote.id }) });
  const again = (await app.invoke("get_note_detail", { id: doneNote.id })).note;
  check("  완료 기록을 다시 정리해도 할 일이 되지 않음", !again.chips.some((c) => c.kind === "task"), JSON.stringify(again.chips.map((c) => c.kind)));
  // 체크를 되돌리면 완료 기록 삭제 ([완료] 탭에서 체크 해제)
  await app.nav("할 일");
  await sleep(500);
  await app.click("완료", ".seg button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.agenda-row input[aria-label="OO교육청 공문 회신 완료"]')`), { label: "완료 탭", timeout: 6000 });
  await app.evaluate(`document.querySelector('.agenda-row input[aria-label="OO교육청 공문 회신 완료"]').click(), true`);
  await sleep(1000);
  const left = (await app.invoke("list_notes", { filter: { limit: 100 } })).filter((n) => n.from_task_id !== null);
  check("체크를 되돌리면 ‘해야 할 일’로, 완료 기록은 삭제", left.length === 0 && (await app.invoke("list_tasks", { state: "open", dutyId: null })).some((t) => t.title === "OO교육청 공문 회신"), `남은 완료 기록 ${left.length}`);

  // ── 2. 창 닫기 → 트레이, 빠른 기록 창, 종료 ──
  ps(`(Get-Process -Id ${child.pid}).CloseMainWindow() | Out-Null`);
  await sleep(1500);
  const visible = ps(`Add-Type -Name V -Namespace U -MemberDefinition '[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h); [DllImport("user32.dll")] public static extern IntPtr FindWindow(string c, string t);'; [U.V]::IsWindowVisible([U.V]::FindWindow($null, '로컬 AI 교무수첩'))`);
  check("창의 X(닫기) → 프로그램은 켜진 채 창만 숨김", alive(child.pid) && visible === "False", `실행 중 ${alive(child.pid)} · 창 보임 ${visible}`);
  await app.invoke("open_quick_window");
  const quick = await connect(9333, 20000, (t) => t.url.includes("#quick"));
  await waitFor(() => quick.evaluate(`!!document.querySelector('.quick-win textarea')`), { label: "빠른 기록 창", timeout: 10000 });
  const mainHidden = ps(`Add-Type -Name V2 -Namespace U -MemberDefinition '[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h); [DllImport("user32.dll")] public static extern IntPtr FindWindow(string c, string t);'; [U.V2]::IsWindowVisible([U.V2]::FindWindow($null, '로컬 AI 교무수첩'))`);
  check("‘기록하기’ → 작은 입력창만 뜸 (큰 창은 숨긴 채)", mainHidden === "False", `큰 창 보임 ${mainHidden}`);
  await quick.evaluate(`window.__e2e.setValue(document.querySelector('.quick-win textarea'), '트레이에서 적은 빠른 기록')`);
  await quick.evaluate(`document.querySelector('.quick-win textarea').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
  await sleep(1500);
  try {
    quick.close();
  } catch {}
  const saved = (await app.invoke("list_notes", { filter: { limit: 100 } })).find((n) => n.raw_text === "트레이에서 적은 빠른 기록");
  const quickGone = await fetch("http://127.0.0.1:9333/json/list").then((r) => r.json()).then((l) => !l.some((t) => t.url.includes("#quick")));
  check("  적고 Enter → 저장되고 입력창은 닫힘 (AI 정리 대상)", !!saved && quickGone && ["pending", "failed", "processing"].includes(saved.status), `${saved?.status} · 창 닫힘 ${quickGone}`);

  // ── AI 계산 장치 (gguf 를 주면) ──
  if (gguf) {
    await app.invoke("import_model_file", { path: gguf });
    const classifyTime = async (raw) => {
      const t0 = Date.now();
      const n = await app.invoke("create_note", { raw });
      await waitFor(async () => (["classified", "failed"].includes((await app.invoke("get_note_detail", { id: n.id })).note.status) ? true : null), { label: "AI 정리", timeout: 300000, interval: 500 });
      return Date.now() - t0;
    };
    const samples = ["1-1 홍길동 다음 주 월요일까지 진로 보고서 제출 받기", "1-2 전우치 과학 실험 시간에 모둠 친구들에게 실험 순서를 설명함", "금요일 5교시 교직원 회의, 회의 자료 목요일까지 준비"];
    await app.invoke("runtime_start");
    // 모델을 넣으면 앞서 정리 못 한 기록부터 정리하므로, 그게 끝난 뒤에 잰다
    // (모델 없이 '정리 실패'로 남은 기록은 다시 줄 서지 않으므로, 기다리는·정리 중인 기록이 없을 때까지)
    const drain = () =>
      waitFor(async () => ((await app.invoke("list_notes", { filter: { limit: 500 } })).some((n) => n.status === "pending" || n.status === "processing") ? null : true), {
        label: "앞선 기록 정리",
        timeout: 600000,
        interval: 1000,
      });
    await drain();
    let rs = await app.invoke("runtime_status");
    const gpuName = rs.device;
    check("자동: 외장 그래픽카드가 있으면 GPU 로 켬", !!gpuName && gpuName !== "CPU", `${gpuName} · 찾은 장치 ${rs.gpus.map((g) => `${g.name}${g.integrated ? "(내장)" : ""}`).join(", ")}`);
    const gpuTimes = [];
    for (const s of samples) gpuTimes.push(await classifyTime(s));
    const s1 = await app.invoke("get_settings");
    await app.invoke("update_settings", { value: { ...s1, ai_device: "cpu" } });
    await app.invoke("runtime_stop");
    await sleep(1500);
    await app.invoke("runtime_start");
    await drain();
    rs = await app.invoke("runtime_status");
    check("설정 ‘CPU 만’ → CPU 로 켬", rs.device === "CPU", rs.device);
    const cpuTimes = [];
    for (const s of samples) cpuTimes.push(await classifyTime(s));
    const avg = (xs) => Math.round(xs.reduce((x, y) => x + y, 0) / xs.length);
    check("GPU 가 CPU 보다 빠르게 정리 (기록 3건 평균)", avg(gpuTimes) < avg(cpuTimes), `GPU ${avg(gpuTimes)}ms · CPU ${avg(cpuTimes)}ms`);
    await app.invoke("update_settings", { value: { ...(await app.invoke("get_settings")), ai_device: "auto" } });
    await app.invoke("runtime_stop").catch(() => null);
  }

  // 종료하기 (정리 중인 기록이 없으면 묻지 않고 끔)
  // 응답보다 프로그램이 먼저 꺼지므로 기다리지 않고 부른다
  await app.evaluate(`(window.__e2e.invoke('quit_app').catch(() => null), true)`).catch(() => null);
  const exited = await waitFor(() => (alive(child.pid) ? null : true), { label: "종료", timeout: 15000 }).catch(() => false);
  check("‘종료하기’ → 프로그램이 완전히 꺼짐", exited === true, "");
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  await fs.writeFile(path.join(shots, "results-v10c.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
