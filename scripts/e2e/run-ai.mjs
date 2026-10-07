// E2E 2단계: AI 모델 다운로드(일시정지·이어받기)·설치 → 미분류 재분류 → 시나리오 A~D AI 처리 → 초안 → 모델 삭제 → 종료 정리
// 실행: node scripts/e2e/run-ai.mjs <exe> <데이터폴더> <스크린샷폴더> [모델id] [파일로 설치할 gguf 경로]
import { execSync } from "node:child_process";
import fs from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [exe, dataRoot, shots, modelId = "qwen2.5-0.5b", importPath = ""] = process.argv.slice(2);
await fs.mkdir(shots, { recursive: true });
const tag = modelId.replace(/[^a-z0-9.]/gi, "");
const results = [];
const metrics = { model: modelId, notes: [] };
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "✔" : "✘"} ${name}${detail ? " — " + detail : ""}`);
};
const ps = (cmd) => execSync(`powershell -NoProfile -Command "${cmd.replace(/"/g, '\\"')}"`, { encoding: "utf8" }).trim();
const memOf = (pid) => {
  try {
    const out = ps(`$p = Get-Process -Id ${pid} -ErrorAction Stop; '{0},{1}' -f $p.WorkingSet64, $p.PrivateMemorySize64`);
    const [ws, priv] = out.split(",").map(Number);
    return { working_set_mb: Math.round(ws / 1048576), private_mb: Math.round(priv / 1048576) };
  } catch {
    return null;
  }
};
const alive = (pid) => {
  try {
    return ps(`if (Get-Process -Id ${pid} -ErrorAction SilentlyContinue) { 'yes' } else { 'no' }`) === "yes";
  } catch {
    return false;
  }
};

const child = launch(exe, { dataRoot });
let app;
let runtimePid = null;
try {
  app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });

  // 모델 설치 전에 쓴 기록 (시나리오 E 후반)
  const before = await app.invoke("create_note", { raw: "어제 1-2 임꺽정 결석. 다음 주 월요일까지 결석계 받기." });
  await waitFor(async () => (await app.invoke("get_note_detail", { id: before.id })).note.status === "failed", { label: "설치 전 미분류" });

  // ── 모델 설치 ──
  await app.nav("설정");
  await waitFor(() => app.evaluate(`!!document.querySelector('.model-card[data-model="${modelId}"]')`), { label: "모델 카드" });
  const t0 = Date.now();
  if (importPath) {
    // 파일로 설치 (명령 직접 호출: 파일 선택 창은 자동화하지 않음)
    const id = await app.invoke("import_model_file", { path: importPath });
    check("파일로 설치 (크기·SHA-256 검증 후 복사)", id === modelId, `${id} ${(Date.now() - t0) / 1000}s`);
  } else {
    await app.evaluate(`document.querySelector('.model-card[data-model="${modelId}"] .mc-actions button.primary').click()`);
    await waitFor(async () => (await app.invoke("model_catalog")).entries.find((e) => e.model.id === modelId)?.progress?.received > 5e7, { label: "다운로드 시작", timeout: 120000 });
    await app.screenshot(path.join(shots, `20-${tag}-downloading.png`));
    await app.invoke("pause_download", { id: modelId });
    const paused = await waitFor(async () => {
      const e = (await app.invoke("model_catalog")).entries.find((x) => x.model.id === modelId);
      return e.progress?.status === "paused" ? e : null;
    }, { label: "일시정지" });
    check("다운로드 일시정지 (.part 유지)", paused.part_bytes > 0, `${Math.round(paused.part_bytes / 1e6)}MB`);
    await sleep(500);
    await app.screenshot(path.join(shots, `21-${tag}-paused.png`));
    await app.invoke("start_download", { id: modelId });
    const installed = await waitFor(async () => {
      const e = (await app.invoke("model_catalog")).entries.find((x) => x.model.id === modelId);
      return e.installed ? e : null;
    }, { label: "이어받기 후 설치", timeout: 1800000, interval: 1000 });
    check("이어받기 → 크기·SHA-256 검증 → 설치", installed.installed && installed.verified_at, `${Math.round((Date.now() - t0) / 1000)}s`);
    metrics.download_seconds = Math.round((Date.now() - t0) / 1000);
  }
  const cat = await app.invoke("model_catalog");
  check("설치한 모델이 자동 선택됨", cat.selected === modelId, cat.selected);
  check("모델 파일이 LocalAppData(데이터 폴더)에 있음", existsSync(path.join(dataRoot, "models", cat.entries.find((e) => e.model.id === modelId).model.file_name)));
  await sleep(600);
  await app.screenshot(path.join(shots, `22-${tag}-installed.png`));

  // ── 설치 전 미분류 기록 → 모두 AI로 분류 ──
  await app.nav("기록");
  await sleep(500);
  await app.click("미분류", ".seg button");
  await sleep(500);
  const tReclass = Date.now();
  await app.click("모두 AI로 분류");
  const reclassified = await waitFor(async () => {
    const d = await app.invoke("get_note_detail", { id: before.id });
    return d.note.status === "classified" ? d : null;
  }, { label: "재분류", timeout: 600000, interval: 500 });
  check("설치 후 과거 미분류 기록을 AI로 다시 분류", !!reclassified, `${Date.now() - tReclass}ms (엔진 시작 포함)`);
  const st = await app.invoke("runtime_status");
  runtimePid = st.state.pid ?? null;
  check("AI 엔진 준비됨", st.state.state === "ready", JSON.stringify(st.state));
  check("AI 엔진이 127.0.0.1 에서만 연결을 받음", st.local_only === true && st.listening.length > 0, st.listening.join(", "));
  metrics.listening = st.listening;
  const bChips = reclassified.note.chips;
  check("재분류 결과: 1-2 임꺽정 출결(결석)", bChips.some((c) => c.kind === "student" && c.label.includes("임꺽정") && c.area === "attendance"), JSON.stringify(bChips));
  check("재분류 결과: 결석계 받기 → 다음 주 월요일(10/5)", bChips.some((c) => c.kind === "task" && c.date === "2026-10-05"), JSON.stringify(bChips));

  // ── 시나리오 A~D 를 AI 로 ──
  await app.invoke("create_duty", { input: { name: "교무기획", detail: "학사일정 관리", guide: "학사력 작성·변경, 수업일수, 교육청 학사일정 보고", keywords: "학사력, 수업일수", active: true } });
  const cases = [
    { id: "A", raw: "오늘 3교시 2-1 홍길동 지각. 담임에게 알리고 금요일까지 보호자 연락." },
    { id: "B", raw: "학생들이 수업 중 손장난을 했고 지적을 받았습니다." },
    { id: "C", raw: "10월 15일 교육장 공감소통 방문" },
    { id: "D1", raw: "2학기 학사일정 변경안이 교무회의에서 확정됨." },
    { id: "D2", raw: "학사력 수정본 금요일까지 교육청에 제출." },
  ];
  const done = {};
  for (const c of cases) {
    const s = Date.now();
    await app.nav("오늘");
    await app.type(".capture textarea", c.raw);
    await app.click("기록", ".capture button");
    const n = await waitFor(async () => (await app.invoke("list_notes", { filter: { limit: 3 } })).find((x) => x.raw_text === c.raw), { label: `저장 ${c.id}` });
    const saveMs = Date.now() - s;
    const d = await waitFor(async () => {
      const x = await app.invoke("get_note_detail", { id: n.id });
      return x.note.status === "classified" || x.note.status === "failed" ? x : null;
    }, { label: `분류 ${c.id}`, timeout: 600000, interval: 300 });
    const run = d.runs[0];
    metrics.notes.push({ case: c.id, save_ms: saveMs, total_ms: Date.now() - s, infer_ms: run?.duration_ms ?? null, status: d.note.status, guard_notes: d.note.guard_notes.length });
    done[c.id] = d;
  }
  const A = done.A.note.chips;
  check("A: 학생 2-1 홍길동 + 출결", A.some((c) => c.kind === "student" && c.label === "2-1 홍길동" && c.area === "attendance"), JSON.stringify(A));
  check("A: 보호자 연락 → 10/2(금) 할 일", A.some((c) => c.kind === "task" && c.label.includes("보호자") && c.date === "2026-10-02"), JSON.stringify(A));
  const B = done.B.note.chips;
  check("B: 출결로 분류하지 않음", B.every((c) => c.area !== "attendance"), JSON.stringify(B));
  check("B: 할 일 만들지 않음", B.every((c) => c.kind !== "task"), JSON.stringify(B));
  const C = done.C.note.chips;
  check("C: 10/15 일정, 원문 그대로", C.some((c) => c.kind === "event" && c.date === "2026-10-15" && c.label === "교육장 공감소통 방문"), JSON.stringify(C));
  check("C: 요약에 꾸민 말 없음", !/제안|방안|협의|논의/.test(done.C.note.summary ?? ""), done.C.note.summary ?? "");
  const D1 = done.D1.note.chips;
  check("D1: 업무 기록만, 할 일 없음", D1.some((c) => c.kind === "duty") && D1.every((c) => c.kind !== "task"), JSON.stringify(D1));
  const D2 = done.D2.note.chips;
  check("D2: 업무 기록 + 기한 있는 할 일(10/2)", D2.some((c) => c.kind === "duty") && D2.some((c) => c.kind === "task" && c.date === "2026-10-02"), JSON.stringify(D2));
  for (const [k, v] of Object.entries(done)) {
    if (v.note.guard_notes.length) console.log(`   안전 규칙(${k}): ${v.note.guard_notes.join(" / ")}`);
    console.log(`   요약(${k}): ${v.note.summary}`);
  }
  if (runtimePid) metrics.memory_llama_server = memOf(runtimePid);
  metrics.memory_app = (() => {
    try {
      const out = ps(`$p = Get-Process teachers_note -ErrorAction Stop | Select-Object -First 1; '{0},{1}' -f $p.WorkingSet64, $p.PrivateMemorySize64`);
      const [ws, pr] = out.split(",").map(Number);
      return { working_set_mb: Math.round(ws / 1048576), private_mb: Math.round(pr / 1048576) };
    } catch {
      return null;
    }
  })();
  await app.nav("기록");
  await sleep(800);
  await app.screenshot(path.join(shots, `23-${tag}-notes-ai.png`));

  // ── AI 초안 ──
  const sid = (await app.invoke("list_students", { query: "홍길동" }))[0]?.id;
  if (sid) {
    const blocks = await app.invoke("record_overview", { studentId: sid });
    const att = blocks.find((b) => b.area === "attendance");
    const ts = Date.now();
    try {
      const draft = await app.invoke("generate_draft_ai", { studentId: sid, area: "attendance", observationIds: att.observations.map((o) => o.id) });
      check("AI 초안 생성 + 근거 연결", draft.source_ids.length > 0 && draft.body.length > 0, `${Date.now() - ts}ms · 근거 밖 표현 ${draft.check.novel_words.length}개: ${draft.check.novel_words.join(",")}`);
      metrics.draft_ms = Date.now() - ts;
    } catch (e) {
      check("AI 초안 생성", false, String(e));
    }
    await app.nav("학생부 자료");
    await sleep(500);
    await app.click("홍길동", ".student-pick button");
    await sleep(900);
    await app.screenshot(path.join(shots, `24-${tag}-records.png`));
  }

  // ── 시나리오 G: 모델 삭제 ──
  await app.nav("설정");
  await sleep(800);
  await app.evaluate(`[...document.querySelectorAll('.model-card[data-model="${modelId}"] .mc-actions button')].find(b => b.innerText.includes('삭제')).click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.dialog')`), { label: "삭제 확인 창" });
  await app.click("지우기", ".dialog-foot button");
  const gone = await waitFor(async () => {
    const e = (await app.invoke("model_catalog")).entries.find((x) => x.model.id === modelId);
    return !e.installed ? e : null;
  }, { label: "삭제 반영", timeout: 60000 });
  await sleep(600);
  const cardText = await app.evaluate(`document.querySelector('.model-card[data-model="${modelId}"]').innerText`);
  check("G: 화면에서 즉시 '설치 안 됨'", cardText.includes("설치 안 됨") && !cardText.includes("사용 중"), cardText.split("\n")[0]);
  check("G: 모델 파일 제거", !existsSync(path.join(dataRoot, "models", gone.model.file_name)));
  const cat3 = await app.invoke("model_catalog");
  check("G: 선택 해제", cat3.selected === null, String(cat3.selected));
  const st3 = await app.invoke("runtime_status");
  check("G: 쓰던 모델이면 AI 엔진 종료", st3.state.state === "stopped" && (!runtimePid || !alive(runtimePid)), JSON.stringify(st3.state));
  await app.screenshot(path.join(shots, `25-${tag}-deleted.png`));
  const aiText = await app.text(".ai-status");
  check("G: 사이드바도 'AI 모델 없음'", aiText.includes("AI 모델 없음"), aiText.replace(/\n/g, " "));
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  await sleep(1000);
  await fs.writeFile(path.join(shots, `results-ai-${tag}.json`), JSON.stringify({ results, metrics }, null, 2));
  console.log(JSON.stringify(metrics, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
