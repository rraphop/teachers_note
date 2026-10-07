// 0.4 E2E
//  A. 업데이트: 0.3 에서 기록·학생·AI 모델 설치 → 같은 데이터 폴더로 0.4 실행 → 그대로인지 (모델은 SHA-256 다시 검사)
//  B. 아래로 옮긴 기록 입력칸
//  C. 학년도 마감 기록물 (화면에서) — PDF/A 두 개, 엑셀, 백업, 읽어보기, SHA256SUMS
//  D. 새 학년도 시작 (화면에서) — 학생 보관, 학급 목록 정리, 새 학급은 새 학년도로, '새 학년도 전' 백업
// 실행: node scripts/e2e/run-v04.mjs <0.4 exe> <0.3 exe> <데이터폴더> <스크린샷폴더> <기록물폴더> <0.5B gguf>
import fs from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [exeNew, exeOld, dataRoot, shots, archiveDir, gguf] = process.argv.slice(2);
await fs.mkdir(shots, { recursive: true });
await fs.mkdir(archiveDir, { recursive: true });
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "OK " : "NG "} ${name}${detail ? " — " + detail : ""}`);
};
const sha = async (p) => crypto.createHash("sha256").update(await fs.readFile(p)).digest("hex");

async function start(exe) {
  const child = launch(exe, { dataRoot });
  const exited = new Promise((res) => child.on("exit", res));
  const app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  return { child, exited, app };
}
async function stop(s) {
  try {
    s.app.close();
  } catch {}
  s.child.kill();
  await Promise.race([s.exited, sleep(8000)]);
  await sleep(1200);
}

let cur = null;
try {
  // ── A-1. 0.3 에서 쓰기 ──
  cur = await start(exeOld);
  let app = cur.app;
  const v3 = (await app.invoke("app_info")).version;
  check("업데이트 전: 0.3 실행", v3 === "0.3.0", v3);
  const kim = await app.invoke("create_student", { input: { name: "홍길동", class_label: "2-1", number: 3 } });
  await app.invoke("set_student_field", { id: kim, field: "phone", value: "010-1234-5678" });
  await app.invoke("create_student", { input: { name: "성춘향", class_label: "2-2", number: 7 } });
  await app.invoke("create_note", { raw: "홍길동 3교시 발표를 잘함, 칭찬함" });
  await app.invoke("create_event", { input: { layer: "annual", title: "2학기 기말고사", start_date: "2026-12-14", end_date: "2026-12-18", time: null, date_expr: null, memo: null } });
  await app.invoke("create_task", { input: { title: "가정통신문 회수", due_date: "2026-10-02", due_time: null, date_expr: null, duty_id: null } });
  const t0 = Date.now();
  const modelId = await app.invoke("import_model_file", { path: gguf });
  check("0.3 에서 AI 모델 파일로 설치 (크기·SHA 검사)", modelId === "qwen2.5-0.5b", `${modelId} ${((Date.now() - t0) / 1000).toFixed(1)}초`);
  await app.invoke("select_model", { id: modelId }).catch(() => undefined);
  await stop(cur);
  cur = null;

  // ── A-2. 같은 데이터로 0.4 실행 ──
  cur = await start(exeNew);
  app = cur.app;
  const v4 = (await app.invoke("app_info")).version;
  check("업데이트 후: 0.4 이상 실행", /^0\.([4-9]|\d{2,})\./.test(v4), v4);
  const notes = await app.invoke("list_notes", { filter: { status: "all" } });
  const studs = await app.invoke("list_students", {});
  const kimAfter = studs.find((s) => s.name === "홍길동");
  check("업데이트 후: 기록·학생·연락처 그대로", notes.some((n) => n.raw_text.includes("발표를 잘함")) && studs.length === 2 && kimAfter?.phone === "010-1234-5678", `기록 ${notes.length}, 학생 ${studs.length}`);
  const cat = await app.invoke("model_catalog");
  const entry = cat.entries.find((e) => e.model.id === "qwen2.5-0.5b");
  const file = path.join(dataRoot, "models", "qwen2.5-0.5b-instruct-q4_k_m.gguf");
  check("업데이트 후: AI 모델 설치 상태·선택 그대로", entry?.installed && cat.selected === "qwen2.5-0.5b" && existsSync(file), `installed=${entry?.installed} selected=${cat.selected}`);
  const verify = await app.invoke("verify_model", { id: "qwen2.5-0.5b" });
  check("업데이트 후: AI 모델 파일 SHA-256 다시 검사 통과", verify?.ok === true, verify?.message ?? "");

  // ── B. 아래 기록 입력칸 ──
  const layout = await app.evaluate(`(() => {
    const cap = document.querySelector('.capture'), box = document.querySelector('.capture-box');
    const r = box.getBoundingClientRect();
    const main = document.querySelector('.main');
    return { bottomGap: Math.round(window.innerHeight - r.bottom), last: main.lastElementChild === cap, sticky: getComputedStyle(cap).position };
  })()`);
  check("기록 입력칸이 화면 아래에 붙어 있음", layout.last && layout.sticky === "sticky" && layout.bottomGap >= 0 && layout.bottomGap < 80, JSON.stringify(layout));
  await app.type(".capture textarea", "금요일까지 체험학습 보고서 제출");
  await app.click("기록", ".capture button");
  await sleep(700);
  const notes2 = await app.invoke("list_notes", { filter: { status: "all" } });
  check("아래 입력칸에서 기록 저장", notes2.length === notes.length + 1, `${notes.length} → ${notes2.length}`);
  await app.screenshot(path.join(shots, "47-capture-bottom.png"));

  // ── C. 학년도 마감 기록물 ──
  await app.nav("설정");
  await sleep(800);
  await app.evaluate(`window.__tnTestPickPath = ${JSON.stringify(archiveDir)}; document.getElementById('set-year').scrollIntoView(); true`);
  await sleep(300);
  await app.screenshot(path.join(shots, "48-year-panel.png"));
  await app.evaluate(`(() => { const tr = [...document.querySelectorAll('.year-table tbody tr')].find(r => r.innerText.includes('2026학년도')); tr.querySelector('button').click(); return true; })()`);
  await waitFor(() => app.evaluate(`(document.querySelector('.dialog')?.innerText || '').includes('마감 기록물 만들기')`), { label: "마감 창" });
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.dialog input[placeholder="예: OO중학교"]'), 'OO중학교')`);
  await sleep(200);
  await app.screenshot(path.join(shots, "49-year-end-dialog.png"));
  const t1 = Date.now();
  await app.click("폴더 고르고 만들기", ".dialog-foot button");
  await waitFor(() => app.evaluate(`(document.querySelector('.dialog')?.innerText || '').includes('마감 기록물을 만들었습니다')`), { timeout: 90000, label: "기록물 결과" });
  const secs = ((Date.now() - t1) / 1000).toFixed(1);
  await app.screenshot(path.join(shots, "50-year-end-done.png"));
  const folder = path.join(archiveDir, "2026학년도 교무수첩 기록물");
  const names = existsSync(folder) ? (await fs.readdir(folder)).sort() : [];
  const want = ["1. 학사·업무 기록 (2026학년도).pdf", "2. 학생 기록 (2026학년도).pdf", "3. 엑셀 원본 (2026학년도).xlsx", "4. 교무수첩 백업 (2026학년도 마감).db", "SHA256SUMS.txt", "읽어보기.txt"];
  check("기록물 폴더: PDF 2·엑셀·백업·읽어보기·SHA256SUMS", want.every((w) => names.includes(w)), `${names.join(", ")} (${secs}초)`);
  for (const pdfName of want.slice(0, 2)) {
    const p = path.join(folder, pdfName);
    if (!existsSync(p)) continue;
    const buf = await fs.readFile(p);
    const text = buf.toString("latin1");
    check(`${pdfName}: PDF/A 표시(pdfaid)`, text.startsWith("%PDF-") && text.includes("pdfaid"), `${Math.round(buf.length / 1024)}KB`);
  }
  if (existsSync(path.join(folder, "SHA256SUMS.txt"))) {
    const sums = (await fs.readFile(path.join(folder, "SHA256SUMS.txt"), "utf8")).replace(/^﻿/, "").trim().split(/\r?\n/);
    let okAll = sums.length === 5;
    for (const line of sums) {
      const [h, n] = [line.slice(0, 64), line.slice(66)];
      okAll = okAll && (await sha(path.join(folder, n))) === h;
    }
    check("SHA256SUMS 값이 실제 파일과 일치", okAll, `${sums.length}줄`);
  }
  const ys = await app.invoke("school_years");
  check("학년도 목록에 마감 기록 남음", !!ys.find((y) => y.school_year === 2026)?.archived_at, "");
  await app.evaluate(`[...document.querySelectorAll('.dialog-foot button')].find(b => b.innerText.includes('닫기')).click()`);
  await sleep(400);

  // ── D. 새 학년도 시작 ──
  await app.click("새 학년도 시작", "#set-year button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.dialog select')`), { label: "새 학년도 창" });
  await app.evaluate(`window.__e2e.setValue(document.querySelector('.dialog select'), '2027')`);
  await waitFor(() => app.evaluate(`(document.querySelector('.dialog')?.innerText || '').includes('학생 2명 보관 처리')`), { label: "2027 미리보기", timeout: 10000 }).catch(async (e) => {
    await app.screenshot(path.join(shots, "51-new-year-dialog-fail.png"));
    throw new Error(`${e.message} / 창 내용: ${(await app.text(".dialog")).replace(/\n/g, " ").slice(0, 300)}`);
  });
  const dlg = await app.text(".dialog");
  check("새 학년도 창: 지난 학년도 마감 기록물 확인·학생 2명 보관 안내", dlg.includes("2026학년도 마감 기록물을") && dlg.includes("만들었습니다") && dlg.includes("2-1"), dlg.replace(/\n/g, " ").slice(0, 150));
  await app.screenshot(path.join(shots, "51-new-year-dialog.png"));
  await app.click("2027학년도 시작", ".dialog-foot button");
  await waitFor(() => app.evaluate(`!document.querySelector('.dialog')`), { label: "새 학년도 창 닫힘" });
  await sleep(600);
  const active = await app.invoke("list_students", {});
  const classes = await app.invoke("list_classes");
  check("새 학년도: 지난 학생 보관(목록에서 숨김), 지난 학급 목록 정리", active.length === 0 && classes.every((c) => c.school_year >= 2027), `학생 ${active.length}, 학급 ${classes.map((c) => c.label).join(",")}`);
  const sem = await app.invoke("current_semester");
  check("새 학년도: 시간표 기본 학기 = 2027학년도 1학기", sem.school_year === 2027 && sem.semester === 1, JSON.stringify(sem));
  await app.invoke("create_student", { input: { name: "새학생", class_label: "1-1", number: 1 } });
  const cls2 = await app.invoke("list_classes");
  check("새 학년도에 넣은 학급은 2027학년도", cls2.some((c) => c.label === "1-1" && c.school_year === 2027), JSON.stringify(cls2.map((c) => [c.label, c.school_year])));
  const backups = await app.invoke("list_backups");
  check("'새 학년도 전' 백업 생성", backups.some((b) => b.kind.startsWith("new-year")), backups.map((b) => b.kind).join(","));
  const kept = await app.invoke("export_data", { options: { format: "csv", path: archiveDir, sheets: ["roster"], school_year: 2026 } });
  const rosterCsv = await fs.readFile(path.join(kept.path, "학급명렬표.csv"), "utf8");
  check("보관한 학생도 2026학년도 자료로는 그대로 남음", rosterCsv.includes("홍길동") && rosterCsv.includes("성춘향"), "");

  // ── 업데이트 기록 ──
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.changelog')`), { label: "업데이트 기록" });
  const log = await app.text(".dialog");
  check("업데이트 기록: v1.0 기능 소개 (학년도 마감)", log.includes("v1.0") && log.includes("학년도 마감"), "");
  await app.screenshot(path.join(shots, "52-update-log-v04.png"));
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  if (cur) await stop(cur);
  await fs.writeFile(path.join(shots, "results-v04.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
