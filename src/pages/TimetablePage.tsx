import { type CSSProperties, useEffect, useMemo, useRef, useState } from "react";
import { api, errorText } from "../api";
import { ErrorBox } from "../components/Fields";
import { IconPlus, IconTrash } from "../components/Icons";
import { confirmDialog, Modal } from "../components/Modal";
import { useLoad, useStore } from "../lib/store";
import { fmtDateTime } from "../lib/format";
import { textOn, TT_PALETTE, ttHex } from "../lib/palette";
import type { AfterSchool, BellConfig, BellSchedule, PeriodSlot, TimetableCell, TimetableKind, TimetableView, TtColors } from "../types";

const WD = ["", "월", "화", "수", "목", "금", "토"];

/** 이 요일에 있는 교시인가 (방과후는 정한 요일에만, 요일을 정하지 않았으면 매일) */
function onWeekday(p: PeriodSlot, wd: number): boolean {
  return !p.days || p.days.length === 0 || p.days.includes(wd);
}

function daysText(days: number[] | undefined): string {
  return days && days.length ? days.map((d) => WD[d]).join("·") : "매일";
}
type Tab = TimetableKind | "bell";

// ───────────── 기록 입력창에서 온 시간표 확인 ─────────────

/** 입력창에 적은 시간표 문장 중 이미 다른 수업이 있던 칸: 바꿀지 교사가 정한다 */
function TtConflicts() {
  const { toast, bump } = useStore();
  const list = useLoad(() => api.ttConflicts(), []);
  if (!list.data || list.data.length === 0) return null;
  const resolve = async (id: number, replace: boolean) => {
    try {
      await api.resolveTtConflict(id, replace);
      toast(replace ? "시간표 칸을 바꾸었습니다." : "원래 수업을 그대로 두었습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <div className="callout soft tt-conflicts">
      <div>
        <strong>기록에서 온 시간표 확인 {list.data.length}건</strong> — 이미 다른 수업이 있는 칸이라 바로 바꾸지 않았습니다.
        <ul>
          {list.data.map((e) => (
            <li key={e.id}>
              <span>
                <b>{e.label}</b> <span className="muted small">(지금: {e.previous} · {e.school_year}학년도 {e.semester}학기)</span>
              </span>
              <button onClick={() => void resolve(e.id, true)}>바꾸기</button>
              <button onClick={() => void resolve(e.id, false)}>그대로</button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

// ───────────── 칸 색 ─────────────

/** 색칠할 칸 */
export interface PaintTarget {
  cellKey: string;
  subject: string;
  teacher: string;
  classLabel: string;
  room: string;
  label: string;
}

/** 칸 색: 이 칸 → 교과 → 교사 → 학급 → 교실 순으로 정한 색 */
function cellColor(colors: TtColors | null, key: string, c: { subject: string; teacher: string; cls: string; room: string }): string | null {
  if (!colors) return null;
  const by = (map: Record<string, string>, k: string) => (k ? ttHex(map[k]) : null);
  return ttHex(colors.cells[key]) ?? by(colors.subjects, c.subject) ?? by(colors.teachers, c.teacher) ?? by(colors.classes, c.cls) ?? by(colors.rooms, c.room);
}

function colorStyle(hex: string | null): CSSProperties | undefined {
  return hex ? { background: hex, color: textOn(hex) } : undefined;
}

type ColorScope = "cell" | "subject" | "teacher" | "class" | "room";

function ColorDialog({ target, colors, onClose, onChanged }: { target: PaintTarget; colors: TtColors | null; onClose: () => void; onChanged: (c: TtColors) => void }) {
  const { toast } = useStore();
  // 순서: 이 칸만 → 교과별 → 교사별 → 학급별 → 교실별 (칸에 그 정보가 있을 때만)
  const scopes: Array<{ scope: ColorScope; key: string; label: string; current?: string }> = [
    { scope: "cell", key: target.cellKey, label: "이 칸만", current: colors?.cells[target.cellKey] },
    ...(target.subject ? [{ scope: "subject" as const, key: target.subject, label: `교과별 (${target.subject})`, current: colors?.subjects[target.subject] }] : []),
    ...(target.teacher ? [{ scope: "teacher" as const, key: target.teacher, label: `교사별 (${target.teacher})`, current: colors?.teachers[target.teacher] }] : []),
    ...(target.classLabel ? [{ scope: "class" as const, key: target.classLabel, label: `학급별 (${target.classLabel})`, current: colors?.classes[target.classLabel] }] : []),
    ...(target.room ? [{ scope: "room" as const, key: target.room, label: `교실별 (${target.room})`, current: colors?.rooms[target.room] }] : []),
  ];
  // 처음에는 교과별 (없으면 이 칸만)
  const [scope, setScope] = useState(scopes.find((s) => s.scope === "subject") ?? scopes[0]!);
  const pick = async (color: string | null) => {
    try {
      onChanged(await api.setTimetableColor(scope.scope, scope.key, color));
      onClose();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <Modal title={`색 정하기 — ${target.label}`} onClose={onClose}>
      <div className="seg color-scopes">
        {scopes.map((s) => (
          <button key={s.scope} className={scope.scope === s.scope ? "on" : ""} onClick={() => setScope(s)}>
            {s.label}
          </button>
        ))}
      </div>
      <p className="small muted">
        색은 ‘이 칸 → 교과 → 교사 → 학급 → 교실’ 순으로 정해집니다. 예를 들어 교과별로 색을 정하면 그 과목 칸이 모두 같은 색이 되고, 한 칸만 다르게 하려면 ‘이 칸만’을 고릅니다. 교과·교사·학급 색은 시간표를 다시 받아도 그대로입니다.
      </p>
      <div className="swatches">
        {TT_PALETTE.map((c) => (
          <button key={c.key} className={`swatch-btn ${scope.current === c.key ? "on" : ""}`} style={{ background: c.hex, color: textOn(c.hex) }} onClick={() => void pick(c.key)} title={c.label}>
            {c.label}
          </button>
        ))}
        <button className="swatch-btn none" onClick={() => void pick(null)} title="색 없음으로 되돌리기">
          없음
        </button>
      </div>
    </Modal>
  );
}
/** 나이스 시간표: 받은 때와 지금 받기 (학교를 정했을 때) */
function NeisTimetableBar() {
  const { toast, bump, navigate } = useStore();
  const st = useLoad(() => api.neisStatus(), []);
  const [busy, setBusy] = useState(false);
  if (!st.data) return null;
  const s = st.data;
  if (!s.school) {
    return (
      <p className="hint">
        설정에서 학교를 정하고 나이스 인증키를 넣으면 학교 시간표를 받아 ‘내 과목’ 칸으로 개인 시간표를 만들 수 있습니다.{" "}
        <button className="link" onClick={() => navigate("settings", { section: "school" })}>
          설정 열기
        </button>
      </p>
    );
  }
  const sync = async () => {
    setBusy(true);
    try {
      const [r] = await api.neisSync("timetable");
      toast(r?.status === "ok" ? `나이스 시간표를 받았습니다 (${r.count}칸).${r.message ? " " + r.message : ""}` : r?.message || "받지 못했습니다.", r?.status === "ok" ? "ok" : "info");
      st.reload();
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const t = s.timetable;
  return (
    <div className="neis-bar">
      <span>
        <b>{s.school.name}</b> 나이스 시간표:{" "}
        {!s.has_key ? "인증키가 있어야 받을 수 있습니다." : t ? `${t.status === "ok" ? "받음" : t.message} · ${fmtDateTime(t.fetched_at)}` : "아직 받지 않음"}
      </span>
      {s.has_key ? (
        <button onClick={() => void sync()} disabled={busy}>
          {busy ? "받는 중…" : "나이스에서 다시 받기"}
        </button>
      ) : (
        <button onClick={() => navigate("settings", { section: "school" })}>인증키 넣기</button>
      )}
      {s.has_key && s.subjects.length > 0 && (
        <button className="link" onClick={() => navigate("settings", { section: "school" })}>
          내 과목 고르기
        </button>
      )}
    </div>
  );
}

// ───────────── 칸 한 줄 입력 ─────────────

/** "2-1 국어 (과학실)" / "국어 김민정 (음악실)" 를 칸으로 바꾼다 */
export function parseCellText(kind: TimetableKind, text: string, base: TimetableCell): TimetableCell {
  let t = text.trim();
  let room = "";
  const m = /\(([^)]*)\)\s*$/.exec(t);
  if (m) {
    room = (m[1] ?? "").trim();
    t = t.slice(0, m.index).trim();
  }
  const tokens = t.split(/\s+/).filter(Boolean);
  const cell: TimetableCell = { ...base, subject: "", teacher: "", class_label: "", room, memo: base.memo };
  if (kind === "personal") {
    if (tokens[0] && /^\d\s*-\s*\d{1,2}$/.test(tokens[0])) cell.class_label = tokens.shift() ?? "";
    cell.subject = tokens.join(" ");
  } else {
    cell.subject = tokens.shift() ?? "";
    cell.teacher = tokens.join(" ");
  }
  return cell;
}

export function cellText(kind: TimetableKind, c: TimetableCell | undefined): string {
  if (!c) return "";
  const main = kind === "personal" ? [c.class_label, c.subject] : [c.subject, c.teacher];
  const s = main.filter(Boolean).join(" ");
  return c.room ? `${s} (${c.room})` : s;
}

interface GridProps {
  kind: TimetableKind;
  year: number;
  semester: number;
  classLabel: string | null;
  view: TimetableView;
  periods: PeriodSlot[];
  weekdays: number[];
  onSaved: () => void;
  colors: TtColors | null;
  /** 색칠하기 중이면 칸을 눌러 색을 고른다 */
  paint: boolean;
  onPaint: (t: PaintTarget) => void;
}

/** 요일 × 교시 표 (개인·학급) */
function WeekGrid({ kind, year, semester, classLabel, view, periods, weekdays, onSaved, colors, paint, onPaint }: GridProps) {
  const { toast } = useStore();
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const rows = periods.filter((p) => p.kind === "class" || p.kind === "after" || p.kind === "lunch");
  const cellRows = rows.filter((p) => p.kind !== "lunch");
  const find = (wd: number, no: number) => view.cells.find((c) => c.weekday === wd && c.period === no);
  useEffect(() => {
    if (editing) inputRef.current?.focus();
  }, [editing]);

  const open = (wd: number, no: number) => {
    setEditing(`${wd}:${no}`);
    setDraft(cellText(kind, find(wd, no)));
  };
  const save = async (wd: number, no: number, move: "down" | "right" | null) => {
    const base: TimetableCell = find(wd, no) ?? { row_label: "", weekday: wd, period: no, subject: "", teacher: "", class_label: "", room: "", memo: "" };
    const next = parseCellText(kind, draft, base);
    try {
      if (draft.trim() !== cellText(kind, find(wd, no))) {
        await api.setTimetableCell(year, semester, kind, classLabel, next);
        onSaved();
      }
    } catch (e) {
      toast(errorText(e), "error");
    }
    if (move === null) {
      setEditing(null);
      return;
    }
    const ri = cellRows.findIndex((p) => p.no === no);
    const wi = weekdays.indexOf(wd);
    if (move === "down") {
      const np = cellRows[ri + 1];
      if (np) open(wd, np.no);
      else setEditing(null);
    } else {
      const nw = weekdays[wi + 1];
      if (nw) open(nw, no);
      else setEditing(null);
    }
  };

  return (
    <div className="tt-scroll">
      <table className="tt-grid">
        <thead>
          <tr>
            <th className="tt-period">교시</th>
            {weekdays.map((w) => (
              <th key={w}>{WD[w]}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((p) =>
            p.kind === "lunch" ? (
              <tr key="lunch" className="tt-lunch">
                <td className="tt-period">
                  점심 <small>{p.start}~{p.end}</small>
                </td>
                <td colSpan={weekdays.length} />
              </tr>
            ) : (
              <tr key={p.no} className={p.kind === "after" ? "tt-after" : ""}>
                <td className="tt-period">
                  <b>{p.label}</b>
                  <small>
                    {p.start}~{p.end}
                  </small>
                  {p.kind === "after" && <small className="tt-days">{daysText(p.days)}</small>}
                </td>
                {weekdays.map((w) => {
                  const key = `${w}:${p.no}`;
                  const c = find(w, p.no);
                  // 방과후를 하지 않는 요일: 비어 있으면 막아 두고, 예전에 적은 칸이 있으면 흐리게 보여 준다 (눌러서 지울 수 있게)
                  if (!onWeekday(p, w) && !c) {
                    return (
                      <td key={key} className="tt-cell tt-off" title={`${WD[w]}요일에는 ${p.label}이 없습니다 (일과 시간 설정에서 요일을 바꿀 수 있습니다)`}>
                        —
                      </td>
                    );
                  }
                  if (editing === key) {
                    return (
                      <td key={key} className="tt-cell editing">
                        <input
                          ref={inputRef}
                          value={draft}
                          onChange={(e) => setDraft(e.target.value)}
                          onBlur={() => void save(w, p.no, null)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") {
                              e.preventDefault();
                              void save(w, p.no, "down");
                            } else if (e.key === "Tab") {
                              e.preventDefault();
                              void save(w, p.no, "right");
                            } else if (e.key === "Escape") setEditing(null);
                          }}
                          placeholder={kind === "personal" ? "2-1 국어 (교실)" : "국어 김민정 (교실)"}
                        />
                      </td>
                    );
                  }
                  const ck = `${year}-${semester}|${kind}|${classLabel ?? ""}|-|${w}|${p.no}`;
                  const cls = kind === "class" ? classLabel ?? "" : c?.class_label ?? "";
                  const hex = cellColor(colors, ck, { subject: c?.subject ?? "", teacher: c?.teacher ?? "", cls: c ? cls : "", room: c?.room ?? "" });
                  return (
                    <td
                      key={key}
                      className={`tt-cell ${c ? "filled" : ""} ${hex ? "colored" : ""} ${paint ? "painting" : ""} ${onWeekday(p, w) ? "" : "tt-off-filled"}`}
                      style={colorStyle(hex)}
                      onClick={() =>
                        paint
                          ? onPaint({ cellKey: ck, subject: c?.subject ?? "", teacher: c?.teacher ?? "", classLabel: c ? cls : "", room: c?.room ?? "", label: `${WD[w]} ${p.label}` })
                          : open(w, p.no)
                      }
                      title={paint ? "눌러서 색 정하기" : "눌러서 입력 (Enter: 아래 칸, Tab: 오른쪽 칸)"}
                    >
                      {c ? (
                        <>
                          {kind === "personal" && c.class_label && <b className="tt-class">{c.class_label}</b>}
                          <span className="tt-subject">{c.subject}</span>
                          {kind !== "personal" && c.teacher && <small className="tt-teacher">{c.teacher}</small>}
                          {c.room && <small className="tt-room">{c.room}</small>}
                        </>
                      ) : (
                        <span className="ph">+</span>
                      )}
                    </td>
                  );
                })}
              </tr>
            ),
          )}
        </tbody>
      </table>
    </div>
  );
}

/** 전체 시간표: 학급 × (요일·교시) */
function SchoolGrid({ year, semester, view, periods, weekdays, onSaved, colors, paint, onPaint }: Omit<GridProps, "kind" | "classLabel">) {
  const { toast } = useStore();
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [newRow, setNewRow] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const cls = periods.filter((p) => p.kind === "class");
  const find = (row: string, wd: number, no: number) => view.cells.find((c) => c.row_label === row && c.weekday === wd && c.period === no);
  useEffect(() => {
    if (editing) inputRef.current?.focus();
  }, [editing]);
  const cols = weekdays.flatMap((w) => cls.map((p) => ({ w, p })));
  const open = (row: string, w: number, no: number) => {
    setEditing(`${row}|${w}|${no}`);
    setDraft(cellText("school", find(row, w, no)));
  };
  const save = async (row: string, w: number, no: number, move: "right" | "down" | null) => {
    const base: TimetableCell = find(row, w, no) ?? { row_label: row, weekday: w, period: no, subject: "", teacher: "", class_label: "", room: "", memo: "" };
    try {
      if (draft.trim() !== cellText("school", find(row, w, no))) {
        await api.setTimetableCell(year, semester, "school", null, parseCellText("school", draft, base));
        onSaved();
      }
    } catch (e) {
      toast(errorText(e), "error");
    }
    if (move === "right") {
      const i = cols.findIndex((c) => c.w === w && c.p.no === no);
      const n = cols[i + 1];
      if (n) open(row, n.w, n.p.no);
      else setEditing(null);
    } else if (move === "down") {
      const ri = view.rows.indexOf(row);
      const nr = view.rows[ri + 1];
      if (nr) open(nr, w, no);
      else setEditing(null);
    } else setEditing(null);
  };
  const addRow = async () => {
    if (!newRow.trim()) return;
    try {
      await api.addSchoolRow(year, newRow);
      setNewRow("");
      onSaved();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <>
      <div className="tt-scroll">
        <table className="tt-grid school">
          <thead>
            <tr>
              <th rowSpan={2} className="tt-period">
                학급
              </th>
              {weekdays.map((w) => (
                <th key={w} colSpan={cls.length}>
                  {WD[w]}
                </th>
              ))}
            </tr>
            <tr>
              {cols.map(({ w, p }) => (
                <th key={`${w}-${p.no}`} className="tt-sub">
                  {p.no}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {view.rows.map((row) => (
              <tr key={row}>
                <td className="tt-period">
                  <b>{row}</b>
                </td>
                {cols.map(({ w, p }) => {
                  const key = `${row}|${w}|${p.no}`;
                  const c = find(row, w, p.no);
                  if (editing === key) {
                    return (
                      <td key={key} className="tt-cell editing">
                        <input
                          ref={inputRef}
                          value={draft}
                          onChange={(e) => setDraft(e.target.value)}
                          onBlur={() => void save(row, w, p.no, null)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") {
                              e.preventDefault();
                              void save(row, w, p.no, "down");
                            } else if (e.key === "Tab") {
                              e.preventDefault();
                              void save(row, w, p.no, "right");
                            } else if (e.key === "Escape") setEditing(null);
                          }}
                        />
                      </td>
                    );
                  }
                  const ck = `${year}-${semester}|school||${row}|${w}|${p.no}`;
                  const hex = cellColor(colors, ck, { subject: c?.subject ?? "", teacher: c?.teacher ?? "", cls: c ? row : "", room: c?.room ?? "" });
                  return (
                    <td
                      key={key}
                      className={`tt-cell mini ${c ? "filled" : ""} ${hex ? "colored" : ""} ${paint ? "painting" : ""}`}
                      style={colorStyle(hex)}
                      onClick={() =>
                        paint
                          ? onPaint({ cellKey: ck, subject: c?.subject ?? "", teacher: c?.teacher ?? "", classLabel: c ? row : "", room: c?.room ?? "", label: `${row} ${WD[w]} ${p.no}교시` })
                          : open(row, w, p.no)
                      }
                      title={paint ? "눌러서 색 정하기" : c ? cellText("school", c) : "눌러서 입력"}
                    >
                      {c ? (
                        <>
                          <span className="tt-subject">{c.subject}</span>
                          {c.teacher && <small className="tt-teacher">{c.teacher}</small>}
                        </>
                      ) : (
                        ""
                      )}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="row gap">
        <input value={newRow} onChange={(e) => setNewRow(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void addRow()} placeholder="학급 추가 (예: 3-2)" />
        <button onClick={() => void addRow()} disabled={!newRow.trim()}>
          <IconPlus size={14} /> 학급 줄 추가
        </button>
        <small className="muted">칸에 “국어 김민정”처럼 과목과 선생님을 적습니다. Tab: 오른쪽, Enter: 아래 학급.</small>
      </div>
    </>
  );
}

// ───────────── 일과 시간 설정 ─────────────

const PRESETS: Array<{ label: string; config: Partial<BellConfig> }> = [
  { label: "초등학교 (40분)", config: { arrival: "08:40", period_minutes: 40, break_minutes: 10, first_start: "09:00", period_count: 6, lunch_after: 4, lunch_minutes: 50, zero_period: false } },
  { label: "중학교 (45분)", config: { arrival: "08:30", period_minutes: 45, break_minutes: 10, first_start: "09:00", period_count: 7, lunch_after: 4, lunch_minutes: 50, zero_period: false } },
  { label: "고등학교 (50분)", config: { arrival: "08:10", period_minutes: 50, break_minutes: 10, first_start: "08:30", period_count: 7, lunch_after: 4, lunch_minutes: 60 } },
];

function BellEditor({ year, semester, bell, onSaved }: { year: number; semester: number; bell: BellSchedule; onSaved: (b: BellSchedule) => void }) {
  const { toast } = useStore();
  const [cfg, setCfg] = useState<BellConfig>(bell.config);
  const [periods, setPeriods] = useState<PeriodSlot[]>(bell.periods);
  const [manual, setManual] = useState(false);
  const [touched, setTouched] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setCfg(bell.config);
    setPeriods(bell.periods);
    setManual(false);
    setTouched(false);
  }, [bell]);
  // 설정을 바꾸면 교시 시각을 다시 계산해 미리 보여 준다 (연달아 바꿔도 앞의 변경이 사라지지 않게 이전 값에 더한다)
  const update = (fn: (prev: BellConfig) => BellConfig) => {
    setTouched(true);
    setCfg((prev) => fn(prev));
  };
  useEffect(() => {
    if (!touched) return;
    let alive = true;
    api
      .computeBell(cfg)
      .then((p) => {
        if (!alive) return;
        setPeriods(p);
        setError(null);
        setManual(false);
      })
      .catch((e) => alive && setError(errorText(e)));
    return () => {
      alive = false;
    };
  }, [cfg, touched]);
  const set = (patch: Partial<BellConfig>) => update((prev) => ({ ...prev, ...patch }));
  const setAfter = (i: number, patch: Partial<AfterSchool>) =>
    update((prev) => ({ ...prev, after_school: prev.after_school.map((a, j) => (j === i ? { ...a, ...patch } : a)) }));
  /** 0교시가 켜져 있으면 1교시 바로 앞(수업 시간 + 쉬는 시간 전)에서 시작하도록 맞춘다 */
  const fitZero = (c: BellConfig): BellConfig => {
    if (!c.zero_period) return c;
    const [h, m] = c.first_start.split(":").map(Number);
    const start = (h ?? 8) * 60 + (m ?? 30) - c.period_minutes - c.break_minutes;
    return start > 0 ? { ...c, zero_start: `${String(Math.floor(start / 60)).padStart(2, "0")}:${String(start % 60).padStart(2, "0")}` } : c;
  };
  const toggleZero = (on: boolean) => update((prev) => fitZero({ ...prev, zero_period: on }));
  const save = async () => {
    try {
      onSaved(await api.saveBell(year, semester, cfg, periods));
      toast("일과 시간을 저장했습니다.");
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const num = (v: string, d: number) => (Number.isFinite(Number(v)) && v !== "" ? Number(v) : d);
  /** 방과후 요일 체크에 보일 요일 (토요일 수업이면 토요일도) */
  const dayList = cfg.saturday ? [1, 2, 3, 4, 5, 6] : [1, 2, 3, 4, 5];
  return (
    <div className="bell">
      <div className="row gap wrap">
        <span className="muted small">빠른 설정:</span>
        {PRESETS.map((p) => (
          <button key={p.label} onClick={() => update((prev) => fitZero({ ...prev, ...p.config }))}>
            {p.label}
          </button>
        ))}
      </div>
      <div className="bell-grid">
        <label className="field">
          <span>등교 시각</span>
          <input type="time" value={cfg.arrival} onChange={(e) => set({ arrival: e.target.value })} />
        </label>
        <label className="field">
          <span>1교시 시작</span>
          <input type="time" value={cfg.first_start} onChange={(e) => set({ first_start: e.target.value })} />
        </label>
        <label className="field">
          <span>수업 시간</span>
          <select value={[40, 45, 50].includes(cfg.period_minutes) ? String(cfg.period_minutes) : "custom"} onChange={(e) => e.target.value !== "custom" && set({ period_minutes: Number(e.target.value) })}>
            <option value="40">40분</option>
            <option value="45">45분</option>
            <option value="50">50분</option>
            <option value="custom">직접 입력</option>
          </select>
          {![40, 45, 50].includes(cfg.period_minutes) && <input type="number" min={20} max={120} value={cfg.period_minutes} onChange={(e) => set({ period_minutes: num(e.target.value, 45) })} />}
        </label>
        <label className="field">
          <span>쉬는 시간</span>
          <select value={[5, 10, 15].includes(cfg.break_minutes) ? String(cfg.break_minutes) : "custom"} onChange={(e) => e.target.value !== "custom" && set({ break_minutes: Number(e.target.value) })}>
            <option value="5">5분</option>
            <option value="10">10분</option>
            <option value="15">15분</option>
            <option value="custom">직접 입력</option>
          </select>
          {![5, 10, 15].includes(cfg.break_minutes) && <input type="number" min={0} max={60} value={cfg.break_minutes} onChange={(e) => set({ break_minutes: num(e.target.value, 10) })} />}
        </label>
        <label className="field">
          <span>교시 수</span>
          <select value={cfg.period_count} onChange={(e) => set({ period_count: Number(e.target.value), lunch_after: Math.min(cfg.lunch_after, Number(e.target.value)) })}>
            {Array.from({ length: 10 }, (_, i) => i + 1).map((n) => (
              <option key={n} value={n}>
                {n}교시까지
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>점심 위치</span>
          <select value={cfg.lunch_after} onChange={(e) => set({ lunch_after: Number(e.target.value) })}>
            <option value={0}>점심 없음</option>
            {Array.from({ length: cfg.period_count }, (_, i) => i + 1).map((n) => (
              <option key={n} value={n}>
                {n}교시 뒤
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>점심 시간</span>
          <select value={cfg.lunch_minutes} onChange={(e) => set({ lunch_minutes: Number(e.target.value) })} disabled={cfg.lunch_after === 0}>
            {[30, 40, 50, 60, 70, 80, 90].map((n) => (
              <option key={n} value={n}>
                {n >= 60 ? `${Math.floor(n / 60)}시간${n % 60 ? ` ${n % 60}분` : ""}` : `${n}분`}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>하교 시각 (비우면 자동)</span>
          <input type="time" value={cfg.dismissal} onChange={(e) => set({ dismissal: e.target.value })} />
        </label>
      </div>
      <div className="row gap wrap">
        <label className="check">
          <input type="checkbox" checked={cfg.zero_period} onChange={(e) => toggleZero(e.target.checked)} /> 0교시 사용
        </label>
        {cfg.zero_period && (
          <label className="inline">
            0교시 시작 <input type="time" value={cfg.zero_start} onChange={(e) => set({ zero_start: e.target.value })} />
          </label>
        )}
        <label className="check">
          <input type="checkbox" checked={cfg.saturday} onChange={(e) => set({ saturday: e.target.checked })} /> 토요일 수업
        </label>
      </div>
      <fieldset className="after-school">
        <legend>방과후</legend>
        {cfg.after_school.length === 0 && <p className="muted small">방과후 시간이 없습니다.</p>}
        {cfg.after_school.map((a, i) => (
          <div key={i} className="row gap wrap after-row">
            <input value={a.name} onChange={(e) => setAfter(i, { name: e.target.value })} placeholder={`방과후 ${i + 1}`} />
            <input type="time" value={a.start} onChange={(e) => setAfter(i, { start: e.target.value })} />~
            <input type="time" value={a.end} onChange={(e) => setAfter(i, { end: e.target.value })} />
            <span className="after-days" role="group" aria-label={`${a.name || `방과후 ${i + 1}`} 요일`}>
              {dayList.map((d) => {
                const days = a.days && a.days.length ? a.days : dayList;
                const on = days.includes(d);
                return (
                  <label key={d} className={`day-check ${on ? "on" : ""}`}>
                    <input
                      type="checkbox"
                      checked={on}
                      onChange={(e) => {
                        const next = e.target.checked ? [...days, d] : days.filter((x) => x !== d);
                        if (next.length === 0) {
                          toast("요일을 하나 이상 골라 주세요. 방과후를 없애려면 오른쪽 휴지통을 누르세요.", "info");
                          return;
                        }
                        setAfter(i, { days: [...new Set(next)].sort((x, y) => x - y) });
                      }}
                    />
                    {WD[d]}
                  </label>
                );
              })}
            </span>
            <button className="icon-btn" onClick={() => update((prev) => ({ ...prev, after_school: prev.after_school.filter((_, j) => j !== i) }))} aria-label="방과후 빼기">
              <IconTrash size={15} />
            </button>
          </div>
        ))}
        <button
          onClick={() => {
            const dismissal = periods.find((p) => p.kind === "dismissal")?.start || "16:40";
            update((prev) => {
              const last = prev.after_school[prev.after_school.length - 1];
              const start = last?.end ?? dismissal;
              const [h, m] = start.split(":").map(Number);
              const endMin = (h ?? 16) * 60 + (m ?? 40) + 50;
              const end = `${String(Math.min(23, Math.floor(endMin / 60))).padStart(2, "0")}:${String(endMin % 60).padStart(2, "0")}`;
              return { ...prev, after_school: [...prev.after_school, { name: `방과후 ${prev.after_school.length + 1}`, start, end, days: dayList }] };
            });
          }}
        >
          <IconPlus size={14} /> 방과후 추가
        </button>
      </fieldset>
      {error && <p className="error-box">{error}</p>}
      {(() => {
        // 방과후가 정규 수업과 겹치면 알린다 (정규 수업은 매일 있으므로)
        const lastClass = [...periods].reverse().find((p) => p.kind === "class");
        const clash = periods.filter((p) => p.kind === "after" && lastClass && p.start < lastClass.end);
        return clash.length > 0 && lastClass ? (
          <p className="hint warn">
            {clash.map((p) => p.label).join(", ")} 시각이 정규 수업({lastClass.label} {lastClass.end}까지)과 겹칩니다. 방과후를 7·8교시처럼 쓰면 ‘교시 수’를 정규 수업까지만(예: 6교시까지) 줄여 주세요.
          </p>
        ) : null;
      })()}
      {(() => {
        const firstClass = periods.find((p) => p.kind === "class");
        return cfg.arrival && firstClass && cfg.arrival > firstClass.start ? (
          <p className="hint warn">
            등교 시각({cfg.arrival})이 첫 수업({firstClass.label} {firstClass.start})보다 늦습니다. 0교시가 있으면 등교 시각을 앞당겨 주세요.
          </p>
        ) : null;
      })()}
      <h3>
        교시별 시각 {manual && <span className="tag warn">직접 고침</span>}
      </h3>
      <table className="bell-table">
        <tbody>
          {periods.map((p, i) => (
            <tr key={`${p.kind}-${p.no}`} className={`bk-${p.kind}`}>
              <td>
                {p.label}
                {p.kind === "after" && <small className="muted"> ({daysText(p.days)})</small>}
              </td>
              <td>
                <input
                  type="time"
                  value={p.start}
                  onChange={(e) => {
                    setManual(true);
                    setPeriods(periods.map((x, j) => (j === i ? { ...x, start: e.target.value, end: x.kind === "arrival" || x.kind === "dismissal" ? e.target.value : x.end } : x)));
                  }}
                />
              </td>
              <td>
                {p.kind !== "arrival" && p.kind !== "dismissal" && (
                  <input
                    type="time"
                    value={p.end}
                    onChange={(e) => {
                      setManual(true);
                      setPeriods(periods.map((x, j) => (j === i ? { ...x, end: e.target.value } : x)));
                    }}
                  />
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="hint">위 설정을 바꾸면 시각을 다시 계산합니다. 학교 사정에 맞게 한 교시씩 직접 고친 뒤 저장해도 됩니다.</p>
      <div className="row gap">
        <button className="primary" onClick={() => void save()} disabled={!!error}>
          일과 시간 저장
        </button>
        {!bell.saved && <span className="muted small">아직 저장하지 않은 기본값입니다.</span>}
      </div>
    </div>
  );
}

// ───────────── 화면 ─────────────

const TAB_KEY = "tn.timetable.tab";

export function TimetablePage() {
  const { toast, bump } = useStore();
  const sem = useLoad(() => api.currentSemester(), []);
  const [year, setYear] = useState<number | null>(null);
  const [semester, setSemester] = useState<number>(1);
  const [tab, setTabState] = useState<Tab>(() => {
    try {
      return (window.localStorage.getItem(TAB_KEY) as Tab | null) ?? "personal";
    } catch {
      return "personal";
    }
  });
  const setTab = (t: Tab) => {
    setTabState(t);
    try {
      window.localStorage.setItem(TAB_KEY, t);
    } catch {
      /* 저장 못 해도 동작 */
    }
  };
  const [classLabel, setClassLabel] = useState<string>("");
  const [newClass, setNewClass] = useState("");
  const [copying, setCopying] = useState(false);
  const [paint, setPaint] = useState(false);
  const [painting, setPainting] = useState<PaintTarget | null>(null);
  const [colors, setColors] = useState<TtColors | null>(null);
  useEffect(() => {
    api.timetableColors().then(setColors).catch(() => setColors(null));
  }, []);
  useEffect(() => {
    if (sem.data && year === null) {
      setYear(sem.data.school_year);
      setSemester(sem.data.semester);
    }
  }, [sem.data, year]);

  const y = year ?? sem.data?.school_year ?? new Date().getFullYear();
  const bell = useLoad(() => api.getBell(y, semester), [y, semester]);
  const classList = useLoad(() => api.classTimetables(y, semester), [y, semester]);
  const kind: TimetableKind = tab === "bell" ? "personal" : tab;
  const view = useLoad(() => api.getTimetable(y, semester, kind, kind === "class" ? classLabel || null : null), [y, semester, kind, classLabel]);
  const weekdays = useMemo(() => (bell.data?.config.saturday ? [1, 2, 3, 4, 5, 6] : [1, 2, 3, 4, 5]), [bell.data]);
  useEffect(() => {
    if (tab === "class" && !classLabel && classList.data && classList.data.length > 0) setClassLabel(classList.data[0] ?? "");
  }, [tab, classLabel, classList.data]);

  const deleteClassTable = async () => {
    if (!classLabel) return;
    if (!(await confirmDialog(`${classLabel} 학급 시간표를 지울까요?`, { okText: "지우기", danger: true }))) return;
    try {
      await api.deleteTimetable(y, semester, "class", classLabel);
      setClassLabel("");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  return (
    <div className="page timetable">
      <div className="page-head">
        <h1>시간표</h1>
        <div className="head-actions">
          <select value={y} onChange={(e) => setYear(Number(e.target.value))} aria-label="학년도">
            {[-1, 0, 1].map((d) => (
              <option key={d} value={(sem.data?.school_year ?? y) + d}>
                {(sem.data?.school_year ?? y) + d}학년도
              </option>
            ))}
          </select>
          <div className="seg">
            {[1, 2].map((s) => (
              <button key={s} className={semester === s ? "on" : ""} onClick={() => setSemester(s)}>
                {s}학기
              </button>
            ))}
          </div>
          <button onClick={() => setCopying(true)}>다른 학기에서 복사</button>
          {tab !== "bell" && (
            <button className={paint ? "primary" : ""} onClick={() => setPaint((x) => !x)} aria-pressed={paint}>
              {paint ? "색칠 끝내기" : "색칠하기"}
            </button>
          )}
        </div>
      </div>
      <TtConflicts />
      <NeisTimetableBar />
      {paint && <p className="callout soft">색칠하기: 칸을 누르면 그 칸·교과·교사·학급·교실의 색을 고를 수 있습니다. 비어 있는 칸(공강)과 점심은 회색으로 보입니다.</p>}
      <p className="hint">아래 기록 입력창에 “월 3교시 2-1 국어 과학실”처럼 적어도 개인 시간표에 들어갑니다. “내일 3교시 2-1 보강”처럼 하루만 바뀌는 것은 그날에만 표시됩니다.</p>
      <div className="seg tabs">
        {(
          [
            ["personal", "개인 시간표"],
            ["class", "학급 시간표"],
            ["school", "전체 시간표"],
            ["bell", "일과 시간 설정"],
          ] as Array<[Tab, string]>
        ).map(([k, l]) => (
          <button key={k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}>
            {l}
          </button>
        ))}
      </div>
      {(bell.error || view.error) && <ErrorBox error={bell.error ?? view.error ?? ""} />}
      {bell.data && !bell.data.saved && tab !== "bell" && (
        <div className="callout soft">
          <span>이 학기의 일과 시간을 아직 정하지 않아 기본값(중학교 45분)으로 보여 줍니다.</span>
          <button onClick={() => setTab("bell")}>일과 시간 설정</button>
        </div>
      )}
      {tab === "bell" && bell.data && <BellEditor year={y} semester={semester} bell={bell.data} onSaved={() => bump()} />}
      {tab === "class" && (
        <div className="row gap wrap tt-classbar">
          <span className="muted small">학급:</span>
          {classList.data?.map((l) => (
            <button key={l} className={classLabel === l ? "on chip-toggle" : "chip-toggle"} onClick={() => setClassLabel(l)}>
              {l}
            </button>
          ))}
          <input value={newClass} onChange={(e) => setNewClass(e.target.value)} placeholder="새 학급 (예: 2-3)" onKeyDown={(e) => e.key === "Enter" && (setClassLabel(newClass.trim()), setNewClass(""))} />
          <button
            onClick={() => {
              setClassLabel(newClass.trim());
              setNewClass("");
            }}
            disabled={!newClass.trim()}
          >
            열기
          </button>
          {classLabel && classList.data?.includes(classLabel) && (
            <button className="ghost danger" onClick={() => void deleteClassTable()}>
              <IconTrash size={14} /> 이 학급 시간표 삭제
            </button>
          )}
        </div>
      )}
      {tab !== "bell" && bell.data && view.data && (tab !== "class" || classLabel) && (
        <>
          {tab === "school" ? (
            <SchoolGrid year={y} semester={semester} view={view.data} periods={bell.data.periods} weekdays={weekdays} onSaved={bump} colors={colors} paint={paint} onPaint={setPainting} />
          ) : (
            <WeekGrid
              kind={kind}
              year={y}
              semester={semester}
              classLabel={tab === "class" ? classLabel : null}
              view={view.data}
              periods={bell.data.periods}
              weekdays={weekdays}
              onSaved={bump}
              colors={colors}
              paint={paint}
              onPaint={setPainting}
            />
          )}
          {tab !== "school" && (
            <p className="hint">
              칸을 눌러 {tab === "personal" ? "“2-1 국어 (과학실)”처럼 학급·과목·(장소)" : "“국어 김민정 (음악실)”처럼 과목·선생님·(장소)"}를 한 줄로 적습니다. Enter는 아래 칸, Tab은 오른쪽 칸으로 넘어갑니다. 지우려면 칸을 비우고 Enter.
            </p>
          )}
        </>
      )}
      {tab === "class" && !classLabel && <p className="muted">학급을 고르거나 새 학급을 적어 시간표를 만드세요.</p>}
      {painting && <ColorDialog target={painting} colors={colors} onClose={() => setPainting(null)} onChanged={setColors} />}
      {copying && (
        <CopyDialog
          toYear={y}
          toSemester={semester}
          onClose={() => setCopying(false)}
          onDone={() => {
            setCopying(false);
            bump();
          }}
        />
      )}
    </div>
  );
}

function CopyDialog({ toYear, toSemester, onClose, onDone }: { toYear: number; toSemester: number; onClose: () => void; onDone: () => void }) {
  const { toast } = useStore();
  const prev = toSemester === 1 ? { y: toYear - 1, s: 2 } : { y: toYear, s: 1 };
  const [fy, setFy] = useState(prev.y);
  const [fs, setFs] = useState(prev.s);
  const run = async () => {
    try {
      const r = await api.copySemester(fy, fs, toYear, toSemester);
      toast(`시간표 ${r.timetables}개(칸 ${r.cells}개)${r.bell ? "와 일과 시간" : ""}을 복사했습니다. 이미 있던 시간표는 그대로 두었습니다.`);
      onDone();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <Modal
      title="다른 학기에서 복사"
      onClose={onClose}
      footer={
        <>
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={() => void run()}>
            복사
          </button>
        </>
      }
    >
      <p>
        가져올 학기를 고르세요. <b>{toYear}학년도 {toSemester}학기</b>로 일과 시간과 시간표를 복사합니다. 이 학기에 이미 있는 시간표는 덮어쓰지 않습니다.
      </p>
      <div className="row gap">
        <select value={fy} onChange={(e) => setFy(Number(e.target.value))}>
          {[toYear - 2, toYear - 1, toYear].map((v) => (
            <option key={v} value={v}>
              {v}학년도
            </option>
          ))}
        </select>
        <select value={fs} onChange={(e) => setFs(Number(e.target.value))}>
          <option value={1}>1학기</option>
          <option value={2}>2학기</option>
        </select>
      </div>
    </Modal>
  );
}
