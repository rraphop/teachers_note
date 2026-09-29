// 기존 구현 평가 (같은 입력·같은 채점): node run-eval-old.mjs <기존 TeacherNotebook.exe> <결과폴더> [세트1,세트2]
// 기존 앱은 %LOCALAPPDATA%\TeacherNotebook 을 쓰므로 반드시 backup-old-data.ps1 로 백업한 뒤 실행하고, 끝나면 복원한다.
import { execSync } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";
import { score, summarize } from "./eval-lib.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const [exe, outDir, sets = "eval-set,eval-holdout"] = process.argv.slice(2);
await fs.mkdir(outDir, { recursive: true });

const AREA = {
  attendance: "attendance",
  subject: "subject",
  creative_autonomy: "autonomous",
  creative_career: "career",
  creative_volunteer: "volunteer",
  creative_club: "club",
  behavior: "behavior",
};
const ps = (cmd) => execSync(`powershell -NoProfile -Command "${cmd.replace(/"/g, '\\"')}"`, { encoding: "utf8" }).trim();
const memOfName = (name) => {
  try {
    const [ws, pr] = ps(`$p = Get-Process ${name} -ErrorAction Stop | Sort-Object WorkingSet64 -Descending | Select-Object -First 1; '{0},{1}' -f $p.WorkingSet64, $p.PrivateMemorySize64`).split(",").map(Number);
    return { working_set_mb: Math.round(ws / 1048576), private_mb: Math.round(pr / 1048576) };
  } catch {
    return null;
  }
};
const listening = () => {
  try {
    return ps(`$p = Get-Process llama-server -ErrorAction Stop | Select-Object -First 1; Get-NetTCPConnection -OwningProcess $p.Id -State Listen | ForEach-Object { '{0}:{1}' -f $_.LocalAddress, $_.LocalPort }`).split(/\r?\n/).filter(Boolean);
  } catch {
    return [];
  }
};

/** 기존 앱 결과 → 채점 형태 */
async function collect(app, noteId) {
  const tasks = (await app.invoke("list_tasks")).filter((t) => t.noteId === noteId);
  const materials = (await app.invoke("list_student_materials", { area: null })).filter((m) => m.noteId === noteId);
  const works = (await app.invoke("list_work_notes")).filter((w) => w.noteId === noteId);
  const got = { students: [], attendanceAny: false, tasks: [], events: [], duty: works.length > 0, studentItems: 0 };
  for (const m of materials) {
    const area = AREA[m.area] ?? m.area;
    got.studentItems++;
    if (area === "attendance") got.attendanceAny = true;
    if (m.studentName) got.students.push({ name: m.studentName, area });
  }
  for (const t of tasks) {
    const due = t.dueAt ?? null;
    const date = due ? due.slice(0, 10) : null;
    const time = due && due.length >= 16 && due.slice(11, 16) !== "00:00" ? due.slice(11, 16) : null;
    const entry = { title: t.content, date, time };
    if (t.taskType === "event") got.events.push(entry);
    else got.tasks.push(entry);
  }
  return { got, raw: { tasks, materials, works } };
}

const child = launch(exe, {});
const app = await connect(9333, 60000);
try {
  await sleep(2000);
  let dutyMade = false;
  for (const setName of sets.split(",")) {
    const set = JSON.parse(await fs.readFile(path.join(here, `${setName}.json`), "utf8"));
    if (!dutyMade) {
      await app.invoke("save_duty_profile", { workName: set.duty.name, details: `${set.duty.detail} — ${set.duty.guide}` });
      dutyMade = true;
    }
    const report = { implementation: "old", set: setName, started: new Date().toISOString(), rows: [] };
    console.log(`\n── ${setName} (기존 구현) ──`);
    for (const item of set.items) {
      const s = Date.now();
      const note = await app.invoke("save_raw_note", { rawText: item.raw });
      const saveMs = Date.now() - s;
      let status = "ok";
      let error = null;
      const t0 = Date.now();
      try {
        await app.invoke("analyze_saved_note", { noteId: note.id });
      } catch (e) {
        status = "failed";
        error = String(e);
      }
      const inferMs = Date.now() - t0;
      const { got, raw } = await collect(app, note.id);
      // 기존 앱은 작성 시각을 UTC(RFC3339)로 저장하므로 현지 날짜로 바꿔 기준일로 쓴다
      const c = new Date(note.createdAt);
      const localDay = `${c.getFullYear()}-${String(c.getMonth() + 1).padStart(2, "0")}-${String(c.getDate()).padStart(2, "0")}`;
      const checks = score(got, item.expect, localDay);
      report.rows.push({ id: item.id, raw: item.raw, status, error, save_ms: saveMs, infer_ms: inferMs, checks, got, detail: raw });
      const ok = checks.filter((c) => c.ok).length;
      console.log(`${ok === checks.length ? "✔" : "✘"} ${item.id} ${ok}/${checks.length} ${inferMs}ms ${error ? "오류:" + error : ""} ${checks.filter((c) => !c.ok).map((c) => `[${c.name}: ${c.detail}]`).join(" ")}`);
    }
    report.memory = { llama_server: memOfName("llama-server"), listening: listening() };
    try {
      report.runtime = await app.invoke("runtime_status");
    } catch {}
    report.summary = summarize(report.rows);
    await fs.writeFile(path.join(outDir, `${setName}-old.json`), JSON.stringify(report, null, 2));
    console.log(JSON.stringify({ set: setName, summary: report.summary, memory: report.memory }, null, 2));
  }
} catch (e) {
  console.log("오류:", e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app.close();
  } catch {}
  // 정상 종료 경로로 닫아 기존 앱의 AI 엔진 정리도 함께 확인한다
  try {
    ps(`$p = Get-Process TeacherNotebook -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '*dist-portable-final*' }; if ($p) { $p.CloseMainWindow() | Out-Null }`);
  } catch {}
  await sleep(5000);
  try {
    child.kill();
  } catch {}
  await sleep(1000);
  process.exit(0);
}
