// 0.3 기능 E2E: 엑셀·CSV 내보내기(설정·명렬표), 연락처 가리기, 백업 되돌리기(재시작 포함), 업데이트 기록
// 실행: node scripts/e2e/run-v03.mjs <exe> <데이터폴더> <스크린샷폴더> <내보내기폴더>
import fs from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import os from "node:os";
import { execFileSync } from "node:child_process";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [exe, dataRoot, shots, exportDir] = process.argv.slice(2);
await fs.mkdir(shots, { recursive: true });
await fs.mkdir(exportDir, { recursive: true });
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "OK " : "NG "} ${name}${detail ? " — " + detail : ""}`);
};
const pad = (n) => String(n).padStart(2, "0");
const now = new Date();
const today = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;

/** xlsx(zip)를 풀어 시트 이름과 문자열 표를 읽는다 */
async function readXlsx(file) {
  const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "tn-xlsx-"));
  const copy = path.join(tmp, "book.zip");
  await fs.copyFile(file, copy);
  execFileSync("tar", ["-xf", copy, "-C", tmp]);
  const workbook = await fs.readFile(path.join(tmp, "xl", "workbook.xml"), "utf8");
  const shared = existsSync(path.join(tmp, "xl", "sharedStrings.xml")) ? await fs.readFile(path.join(tmp, "xl", "sharedStrings.xml"), "utf8") : "";
  const sheets = [...workbook.matchAll(/<sheet [^>]*name="([^"]+)"/g)].map((m) => m[1]);
  await fs.rm(tmp, { recursive: true, force: true });
  return { sheets, shared };
}

/** 재시작으로 새로 뜬 교무수첩(부모가 처음 띄운 프로세스)만 끈다 — 교사가 켜 둔 창은 건드리지 않는다 */
function killChildrenOf(pid) {
  const ps = `Get-CimInstance Win32_Process -Filter "ParentProcessId=${pid}" | Where-Object { $_.Name -eq 'TeacherNotebook.exe' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force; $_.ProcessId }`;
  const enc = Buffer.from(ps, "utf16le").toString("base64");
  try {
    return execFileSync("powershell", ["-NoProfile", "-EncodedCommand", enc], { encoding: "utf8" }).trim();
  } catch {
    return "";
  }
}

const closeDialog = async (app) => {
  await app.evaluate(`(() => { const b = [...document.querySelectorAll('.dialog-foot button')].find(b => /닫기|확인/.test(b.innerText)); b?.click(); return true; })()`);
  await sleep(300);
};

const child = launch(exe, { dataRoot });
const firstPid = child.pid;
const exited = new Promise((res) => child.on("exit", res));
let app;
let restarted = false;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  const info = await app.invoke("app_info");
  check("버전 0.3 이상", /^0\.([3-9]|\d{2,})\./.test(info.version) || /^[1-9]\d*\./.test(info.version), info.version);

  // ── 준비: 명렬표·기록·일정·할 일·시간표 ──
  const kim = await app.invoke("create_student", { input: { name: "홍길동", class_label: "2-1", number: 3 } });
  await app.invoke("create_student", { input: { name: "성춘향", class_label: "2-1", number: 7 } });
  await app.invoke("create_student", { input: { name: "이몽룡", class_label: "2-2", number: 1 } });
  await app.invoke("set_student_field", { id: kim, field: "phone", value: "010-1234-5678" });
  await app.invoke("set_student_field", { id: kim, field: "address", value: "OO도 OO시 OO구 OO로 1" });
  await app.invoke("create_note", { raw: "홍길동 3교시 발표를 잘함, 칭찬함" });
  await app.invoke("create_event", { input: { layer: "annual", title: "2학기 기말고사", start_date: "2026-12-14", end_date: "2026-12-18", time: null, date_expr: null, memo: null } });
  await app.invoke("create_task", { input: { title: "가정통신문 회수", due_date: "2026-10-02", due_time: null, date_expr: null, duty_id: null } });
  const sem = await app.invoke("current_semester");
  await app.invoke("set_timetable_cell", {
    year: sem.school_year,
    semester: sem.semester,
    kind: "personal",
    classLabel: null,
    cell: { row_label: "", weekday: 1, period: 1, subject: "국어", teacher: "", class_label: "2-1", room: "", memo: "" },
  });
  await sleep(500);

  // ── 1. 설정 → 엑셀 파일로 내보내기 ──
  await app.nav("설정");
  await sleep(700);
  await app.evaluate(`window.__tnTestPickPath = ${JSON.stringify(exportDir)}; document.getElementById('set-export').scrollIntoView(); true`);
  await sleep(300);
  const panel = await app.text("#set-export");
  check("설정에 ‘엑셀·CSV로 내보내기’ (항목 9개, 개인정보 경고)", ["학급명렬표", "기록 원문", "학생 관찰", "학생부 초안", "할 일", "일정·학사력", "담당업무", "업무 기록", "시간표"].every((s) => panel.includes(s)) && panel.includes("암호가 없습니다"), "");
  await app.screenshot(path.join(shots, "41-export-panel.png"));
  await app.click("엑셀 파일로 저장", "#set-export button");
  await waitFor(() => app.evaluate(`(document.querySelector('.dialog')?.innerText || '').includes('내보냈습니다')`), { label: "내보내기 결과 창" });
  await app.screenshot(path.join(shots, "42-export-done.png"));
  const xlsxPath = path.join(exportDir, `교무수첩 ${today}.xlsx`);
  check("엑셀 파일 생성", existsSync(xlsxPath), xlsxPath);
  if (existsSync(xlsxPath)) {
    const x = await readXlsx(xlsxPath);
    check("엑셀: 시트 9개 이름", x.sheets.join(",") === "학급명렬표,기록 원문,학생 관찰,학생부 초안,할 일,일정·학사력,담당업무,업무 기록,시간표", x.sheets.join(","));
    check("엑셀: 내용(학생·연락처·기록·학사력·시간표)", ["홍길동", "010-1234-5678", "발표를 잘함", "2학기 기말고사", "국어", "1교시"].every((s) => x.shared.includes(s)), "");
  }
  await closeDialog(app);

  // ── 2. CSV ──
  await app.click("CSV — 항목마다 파일", "#set-export button");
  await sleep(200);
  await app.click("CSV 파일로 저장", "#set-export button");
  await waitFor(() => app.evaluate(`(document.querySelector('.dialog')?.innerText || '').includes('내보냈습니다')`), { label: "CSV 결과 창" });
  await closeDialog(app);
  const dirs = (await fs.readdir(exportDir)).filter((d) => d.startsWith("교무수첩 내보내기 "));
  check("CSV 폴더 하나 생성", dirs.length === 1, dirs.join(","));
  if (dirs.length) {
    const csvDir = path.join(exportDir, dirs[0]);
    const files = (await fs.readdir(csvDir)).filter((f) => f.endsWith(".csv"));
    check("CSV 파일 9개", files.length === 9, files.join(","));
    const roster = await fs.readFile(path.join(csvDir, "학급명렬표.csv"), "utf8");
    check("CSV: 엑셀용 BOM·머리글·내용", roster.startsWith("\ufeff학급,번호,이름,전화번호") && roster.includes("2-1,3,홍길동,010-1234-5678") && roster.includes("\r\n"), roster.slice(0, 80).replace(/\r\n/g, " | "));
    const notesCsv = await fs.readFile(path.join(csvDir, "기록 원문.csv"), "utf8");
    check("CSV: 쉼표 있는 원문은 따옴표로 감쌈", notesCsv.includes('"홍길동 3교시 발표를 잘함, 칭찬함"'), "");
  }

  // ── 3. 학급명렬표 → 엑셀로 저장 (보이는 항목 + 가리기) ──
  await app.nav("학생");
  await sleep(500);
  await app.click("학급명렬표", ".seg button");
  await sleep(600);
  await app.evaluate(`[...document.querySelectorAll('.roster-cols label')].find(l => l.innerText.includes('전화번호')).querySelector('input').click()`);
  await sleep(500);
  await app.evaluate(`[...document.querySelectorAll('.roster-bar label.check')].find(l => l.innerText.includes('가리기')).querySelector('input').click()`);
  await sleep(300);
  await app.screenshot(path.join(shots, "43-roster-export-button.png"));
  await app.click("엑셀로 저장", ".roster-bar button");
  await waitFor(() => app.evaluate(`(document.querySelector('.dialog')?.innerText || '').includes('내보냈습니다')`), { label: "명렬표 내보내기 결과" });
  await closeDialog(app);
  const rosterXlsx = path.join(exportDir, `학급명렬표 전체 ${today}.xlsx`);
  check("명렬표 엑셀 파일 생성", existsSync(rosterXlsx), rosterXlsx);
  if (existsSync(rosterXlsx)) {
    const x = await readXlsx(rosterXlsx);
    check("명렬표 엑셀: 시트 1개, 켜 둔 항목만(주소 없음)", x.sheets.join(",") === "학급명렬표" && x.shared.includes("전화번호") && !x.shared.includes("주소"), x.sheets.join(","));
    check("명렬표 엑셀: 연락처 가림", x.shared.includes("●●●-●●●●-5678") && !x.shared.includes("010-1234-5678"), "");
  }

  // ── 4. 안전 확인 ──
  const reveal = await app.evaluate(`window.__e2e.invoke('reveal_export', { path: 'C:\\\\Windows\\\\System32' }).then(() => 'ok', e => String(e))`);
  check("탐색기 열기는 이번에 내보낸 파일만", reveal !== "ok", reveal);
  const rel = await app.evaluate(`window.__e2e.invoke('export_data', { options: { format: 'csv', path: '상대경로', sheets: ['roster'] } }).then(() => 'ok', e => String(e))`);
  check("상대 경로로는 내보내지 않음", rel !== "ok", rel);
  const none = await app.evaluate(`window.__e2e.invoke('export_data', { options: { format: 'xlsx', path: ${JSON.stringify(path.join(exportDir, "x.xlsx"))}, sheets: [] } }).then(() => 'ok', e => String(e))`);
  check("항목을 고르지 않으면 거절", none !== "ok", none);

  // ── 5. 백업 → 기록 추가 → 되돌리기 (재시작) ──
  await app.invoke("backup_now");
  await sleep(1100); // 백업 이름이 초 단위라 겹치지 않게
  await app.invoke("create_note", { raw: "되돌리면 사라질 기록" });
  await app.nav("설정");
  await sleep(800);
  await app.evaluate(`(() => { const d = document.querySelector('.backup-list'); d.open = true; d.scrollIntoView(); return true; })()`);
  await sleep(300);
  await app.evaluate(`(() => { const tr = [...document.querySelectorAll('.backup-table tbody tr')].find(r => r.innerText.includes('직접')); tr.querySelector('button').click(); return true; })()`);
  await waitFor(() => app.evaluate(`(document.querySelector('.dialog')?.innerText || '').includes('기록 1개')`), { label: "되돌리기 확인 창" });
  const dlg = await app.text(".dialog");
  check("되돌리기 확인 창: 백업 내용·‘되돌리기 전’ 안내", dlg.includes("학생 3명") && dlg.includes("되돌리기 전"), dlg.replace(/\n/g, " ").slice(0, 160));
  await app.screenshot(path.join(shots, "44-restore-dialog.png"));
  await app.click("되돌리고 다시 시작", ".dialog-foot button");
  const code = await Promise.race([exited, sleep(20000).then(() => "timeout")]);
  check("되돌리기 → 프로그램이 스스로 다시 시작(이전 프로세스 종료)", code !== "timeout", String(code));
  restarted = true;
  try {
    app.close();
  } catch {}
  await sleep(1500);
  app = await waitFor(
    async () => {
      const a = await connect(9333, 5000);
      const ok = await a.evaluate(`(document.querySelector('.dialog')?.innerText || '').includes('백업으로 되돌렸습니다')`).catch(() => false);
      if (ok) return a;
      a.close();
      return false;
    },
    { timeout: 40000, label: "다시 시작한 화면의 되돌리기 결과" },
  );
  check("다시 시작 후 ‘백업으로 되돌렸습니다’ 알림", true, "");
  await app.screenshot(path.join(shots, "45-restore-done.png"));
  await closeDialog(app);
  const notes = await app.invoke("list_notes", { filter: { status: "all" } });
  check("되돌린 뒤: 백업 뒤에 쓴 기록은 없음, 백업 전 기록은 있음", notes.length === 1 && notes[0].raw_text.includes("발표를 잘함"), notes.map((n) => n.raw_text).join(" / "));
  const backups = await app.invoke("list_backups");
  const before = backups.find((b) => b.kind.startsWith("before-restore"));
  check("‘되돌리기 전’ 백업이 남음", !!before, backups.map((b) => `${b.taken_at} ${b.kind}`).join(", "));
  if (before) {
    const c = await app.invoke("inspect_backup", { name: before.name });
    check("‘되돌리기 전’ 백업에는 기록 2개(되돌리기 직전 상태)", c.notes === 2, JSON.stringify(c));
  }
  const auto = await waitFor(async () => (await app.invoke("list_backups")).find((b) => b.kind.startsWith("auto")), { timeout: 8000, label: "자동 백업" }).catch(() => null);
  check("다시 시작하면 오늘 자동 백업 생성", !!auto, auto?.name ?? "");

  // ── 6. 업데이트 기록 ──
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.changelog')`), { label: "업데이트 기록" });
  const log = await app.text(".dialog");
  check("업데이트 기록: v0.3 (내보내기·되돌리기)", log.includes("v0.3") && log.includes("엑셀") && log.includes("되돌리기"), "");
  await app.screenshot(path.join(shots, "46-update-log-v03.png"));
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  const killed = killChildrenOf(firstPid);
  if (restarted) console.log(`다시 시작한 시험용 프로세스 종료: ${killed || "(없음)"}`);
  await fs.writeFile(path.join(shots, "results-v03.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
