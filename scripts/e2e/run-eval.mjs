// 새 구현 평가: node run-eval.mjs <exe> <데이터폴더> <결과폴더> <rules|모델id> [download|<gguf 경로>] [세트1,세트2]
// 모델은 한 번만 설치하고 여러 평가 세트를 차례로 돌린다.
import { execSync } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";
import { fromPlan, score, summarize } from "./eval-lib.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const [exe, dataRoot, outDir, mode, source = "download", sets = "eval-set,eval-holdout"] = process.argv.slice(2);
await fs.mkdir(outDir, { recursive: true });

const ps = (cmd) => execSync(`powershell -NoProfile -Command "${cmd.replace(/"/g, '\\"')}"`, { encoding: "utf8" }).trim();
const mem = (pid) => {
  try {
    const [ws, pr] = ps(`$p = Get-Process -Id ${pid} -ErrorAction Stop; '{0},{1}' -f $p.WorkingSet64, $p.PrivateMemorySize64`).split(",").map(Number);
    return { working_set_mb: Math.round(ws / 1048576), private_mb: Math.round(pr / 1048576) };
  } catch {
    return null;
  }
};

const child = launch(exe, { dataRoot });
const app = await connect(9333);
const meta = { implementation: "new", mode, install: null, engine_start_ms: null, memory: null };
try {
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea')`), { label: "첫 화면" });
  if (mode !== "rules") {
    const t0 = Date.now();
    if (source === "download") {
      await app.invoke("start_download", { id: mode });
      await waitFor(async () => (await app.invoke("model_catalog")).entries.find((e) => e.model.id === mode)?.installed, { label: "모델 설치", timeout: 3600000, interval: 2000 });
    } else {
      await app.invoke("import_model_file", { path: source });
    }
    meta.install = { how: source === "download" ? "download" : "import", seconds: Math.round((Date.now() - t0) / 1000) };
    const t1 = Date.now();
    await app.invoke("runtime_start");
    meta.engine_start_ms = Date.now() - t1;
    console.log(`모델 설치 ${meta.install.seconds}s, 엔진 시작 ${meta.engine_start_ms}ms`);
  }
  let dutyMade = false;
  for (const setName of sets.split(",")) {
    const set = JSON.parse(await fs.readFile(path.join(here, `${setName}.json`), "utf8"));
    if (!dutyMade) {
      await app.invoke("create_duty", { input: { ...set.duty, active: true } });
      dutyMade = true;
    }
    const report = { ...meta, set: setName, started: new Date().toISOString(), rows: [] };
    console.log(`\n── ${setName} (${mode}) ──`);
    for (const item of set.items) {
      const s = Date.now();
      const note = await app.invoke("create_note", { raw: item.raw });
      let plan;
      let infer = null;
      let status = "rules";
      let guard = [];
      if (mode === "rules") {
        plan = await app.invoke("suggest_plan", { id: note.id });
      } else {
        const d = await waitFor(async () => {
          const x = await app.invoke("get_note_detail", { id: note.id });
          return x.note.status === "classified" || x.note.status === "failed" ? x : null;
        }, { label: `분류 ${item.id}`, timeout: 900000, interval: 300 });
        plan = d.plan;
        infer = d.runs[0]?.duration_ms ?? null;
        status = d.note.status;
        guard = d.note.guard_notes;
        if (status === "failed") console.log(`   실패 ${item.id}: ${d.note.status_reason}`);
      }
      const checks = score(fromPlan(plan), item.expect, note.created_at.slice(0, 10));
      report.rows.push({
        id: item.id,
        raw: item.raw,
        status,
        infer_ms: infer,
        wall_ms: Date.now() - s,
        summary: plan.summary,
        guard_notes: guard,
        checks,
        items: plan.items.map((i) => ({ kind: i.kind, text: i.text, area: i.area, students: i.students.map((x) => x.name), date: i.date, time: i.time, origin: i.origin })),
      });
      const ok = checks.filter((c) => c.ok).length;
      console.log(`${ok === checks.length ? "✔" : "✘"} ${item.id} ${ok}/${checks.length} ${infer !== null ? infer + "ms" : ""} ${checks.filter((c) => !c.ok).map((c) => `[${c.name}: ${c.detail}]`).join(" ")}`);
    }
    if (mode !== "rules") {
      const st = await app.invoke("runtime_status");
      if (st.state.pid) report.memory = { llama_server: mem(st.state.pid), listening: st.listening, local_only: st.local_only };
    }
    report.summary = summarize(report.rows);
    await fs.writeFile(path.join(outDir, `${setName}-new-${mode}.json`), JSON.stringify(report, null, 2));
    console.log(JSON.stringify({ set: setName, summary: report.summary, memory: report.memory }, null, 2));
  }
} catch (e) {
  console.log("오류:", e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app.close();
  } catch {}
  child.kill();
  await sleep(1000);
  process.exit(0);
}
