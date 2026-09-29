// 평가 세트 채점 (두 구현 공통). 결과를 {students, tasks, events, duty} 형태로 바꿔 넣으면 기대값과 비교한다.

const pad = (n) => String(n).padStart(2, "0");
const iso = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const addDays = (d, n) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
const monday = (d) => addDays(d, -((d.getDay() + 6) % 7));

/**
 * 기대 날짜 표현을 기록 작성일 기준 실제 날짜로 바꾼다 (평가가 자정을 넘겨도 맞게).
 * today, yesterday, tomorrow, d+N, fri(오늘 포함 가장 가까운 금요일), thisweek_fri, next_mon|next_tue|next_wed, nextmonth_N, 또는 YYYY-MM-DD
 */
export function resolveDate(token, baseIso) {
  if (token === undefined || token === null) return token;
  if (/^\d{4}-\d{2}-\d{2}$/.test(token)) return token;
  const [y, m, d] = baseIso.split("-").map(Number);
  const base = new Date(y, m - 1, d);
  if (token === "today") return iso(base);
  if (token === "yesterday") return iso(addDays(base, -1));
  if (token === "tomorrow") return iso(addDays(base, 1));
  let mm = /^d\+(\d+)$/.exec(token);
  if (mm) return iso(addDays(base, Number(mm[1])));
  if (token === "fri") {
    let x = base;
    while (x.getDay() !== 5) x = addDays(x, 1);
    return iso(x);
  }
  if (token === "thisweek_fri") return iso(addDays(monday(base), 4));
  const wd = { next_mon: 0, next_tue: 1, next_wed: 2, next_thu: 3, next_fri: 4 };
  if (token in wd) return iso(addDays(monday(base), 7 + wd[token]));
  mm = /^nextmonth_(\d+)$/.exec(token);
  if (mm) return iso(new Date(base.getFullYear(), base.getMonth() + 1, Number(mm[1])));
  throw new Error(`알 수 없는 날짜 표현: ${token}`);
}

/**
 * @param {{students:{name:string,area:string}[], attendanceAny:boolean, tasks:{title:string,date:string|null}[], events:{title:string,date:string|null,time:string|null}[], duty:boolean, studentItems:number}} got
 * @param {any} expect
 */
export function score(got, rawExpect, baseIso) {
  const fix = (arr) => (arr ?? []).map((x) => ({ ...x, date: x.date === undefined ? undefined : resolveDate(x.date, baseIso) }));
  const expect = { ...rawExpect, tasks: fix(rawExpect.tasks), events: fix(rawExpect.events) };
  const checks = [];
  const add = (name, ok, detail = "") => checks.push({ name, ok: !!ok, detail });
  for (const s of expect.students ?? []) {
    const hit = got.students.some((x) => x.name === s.name && x.area === s.area);
    add(`학생 ${s.name}·${s.area}`, hit, got.students.map((x) => `${x.name}:${x.area}`).join(",") || "(없음)");
  }
  if (expect.no_attendance) add("출결 아님", !got.attendanceAny, got.attendanceAny ? "출결로 분류됨" : "");
  if (expect.no_students) add("학생 관찰 없음", got.studentItems === 0, String(got.studentItems));
  for (const t of expect.tasks ?? []) {
    const hit = got.tasks.some((x) => x.title.includes(t.has) && (t.date === undefined || x.date === t.date));
    add(`할 일 '${t.has}'${t.date ? "@" + t.date : ""}`, hit, got.tasks.map((x) => `${x.title}@${x.date ?? "-"}`).join(", ") || "(없음)");
  }
  if (expect.no_tasks) add("할 일 없음", got.tasks.length === 0, got.tasks.map((x) => x.title).join(", "));
  if (expect.task_count !== undefined) add(`할 일 ${expect.task_count}개`, got.tasks.length === expect.task_count, String(got.tasks.length));
  for (const e of expect.events ?? []) {
    const hit = got.events.some(
      (x) => x.title.includes(e.has) && (e.date === undefined || x.date === e.date) && (e.time === undefined || x.time === e.time),
    );
    add(`일정 '${e.has}'${e.date ? "@" + e.date : ""}${e.time ? " " + e.time : ""}`, hit, got.events.map((x) => `${x.title}@${x.date ?? "-"} ${x.time ?? ""}`).join(", ") || "(없음)");
  }
  if (expect.no_events) add("일정 없음", got.events.length === 0, got.events.map((x) => x.title).join(", "));
  if (expect.duty) add("업무 기록", got.duty, "");
  return checks;
}

/** 새 구현의 분류 계획(Plan) → 채점용 형태 */
export function fromPlan(plan) {
  const got = { students: [], attendanceAny: false, tasks: [], events: [], duty: false, studentItems: 0 };
  for (const it of plan.items) {
    if (it.kind === "student") {
      got.studentItems++;
      if (it.area === "attendance") got.attendanceAny = true;
      for (const s of it.students) got.students.push({ name: s.name, area: it.area });
    } else if (it.kind === "task") got.tasks.push({ title: it.text, date: it.date, time: it.time });
    else if (it.kind === "event") got.events.push({ title: it.text, date: it.date, time: it.time });
    else if (it.kind === "duty") got.duty = true;
  }
  return got;
}

export function summarize(rows) {
  const total = rows.reduce((a, r) => a + r.checks.length, 0);
  const passed = rows.reduce((a, r) => a + r.checks.filter((c) => c.ok).length, 0);
  const perfect = rows.filter((r) => r.checks.every((c) => c.ok)).length;
  const times = rows.map((r) => r.infer_ms).filter((x) => typeof x === "number");
  const avg = times.length ? Math.round(times.reduce((a, b) => a + b, 0) / times.length) : null;
  const max = times.length ? Math.max(...times) : null;
  const falseAttendance = rows.filter((r) => r.checks.some((c) => c.name === "출결 아님" && !c.ok)).length;
  return { passed, total, rate: total ? Math.round((passed / total) * 1000) / 10 : 0, perfect_notes: perfect, notes: rows.length, avg_infer_ms: avg, max_infer_ms: max, false_attendance_notes: falseAttendance };
}
