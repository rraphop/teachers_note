import { useMemo, useState } from "react";
import { api } from "../api";
import { openEditor } from "../components/Editors";
import { ErrorBox } from "../components/Fields";
import { IconLeft, IconPlus, IconRight } from "../components/Icons";
import { addDays, addMonths, fmtTime, isoDate, parseDate, startOfWeek, todayISO, weekdayKo } from "../lib/format";
import { LAYER_HELP, LAYER_LABEL } from "../lib/labels";
import { DAY_KIND_LABEL, DAY_KIND_ORDER, dayHex } from "../lib/palette";
import { useLoad, useStore } from "../lib/store";
import type { CalendarItem, CalendarView, DayColors, DayKind, Layer } from "../types";

const WEEK_HEAD = ["월", "화", "수", "목", "금", "토", "일"];
/** 학사력(연간) 달력은 일요일부터 */
const YEAR_HEAD = ["일", "월", "화", "수", "목", "금", "토"];
const DEFAULT_LAYER: Record<CalendarView, Layer> = { year: "annual", month: "monthly", week: "weekly" };

/** 학년도: 3월에 시작해 이듬해 2월에 끝난다 */
function schoolYearOf(d: Date): number {
  return d.getMonth() >= 2 ? d.getFullYear() : d.getFullYear() - 1;
}

function range(view: CalendarView, anchor: Date, schoolYear: boolean): { start: Date; end: Date } {
  if (view === "week") {
    const s = startOfWeek(anchor);
    return { start: s, end: addDays(s, 6) };
  }
  if (view === "month") {
    const first = new Date(anchor.getFullYear(), anchor.getMonth(), 1);
    const s = startOfWeek(first);
    return { start: s, end: addDays(s, 41) };
  }
  if (schoolYear) {
    const sy = schoolYearOf(anchor);
    return { start: new Date(sy, 2, 1), end: new Date(sy + 1, 2, 0) };
  }
  return { start: new Date(anchor.getFullYear(), 0, 1), end: new Date(anchor.getFullYear(), 11, 31) };
}

/** 그날 보이는 항목 (토요휴업일처럼 색만 칠하는 숨김 학사력은 빼고) */
function itemsOn(items: CalendarItem[], day: string): CalendarItem[] {
  return items.filter((i) => !i.hidden && i.start_date <= day && i.end_date >= day);
}

/** 그날의 학사력 종류 (겹치면 휴일 → 방학 → 휴업일 순). 업무만 있는 날은 색을 칠하지 않는다. */
function dayKindOn(items: CalendarItem[], day: string): DayKind | null {
  const kinds = items.filter((i) => i.day_kind && i.start_date <= day && i.end_date >= day).map((i) => i.day_kind);
  return DAY_KIND_ORDER.find((k) => k !== "work" && kinds.includes(k)) ?? null;
}

function gradeTag(grades: string): string {
  return grades
    .split(",")
    .filter(Boolean)
    .map((g) => `${g}학년`)
    .join("·");
}

let openStudent: ((id: number) => void) | null = null;
let openNote: ((id: number) => void) | null = null;

function openItem(i: CalendarItem) {
  if (i.kind === "task") openEditor({ kind: "task", id: i.id });
  else if (i.kind === "duty_log") openEditor({ kind: "duty_log", id: i.id });
  else if (i.kind === "birthday") openStudent?.(i.id);
  // 시간표 변경은 그 변경을 적은 기록으로
  else if (i.kind === "tt_change") {
    if (i.note_id) openNote?.(i.note_id);
  } else openEditor({ kind: "event", id: i.id });
}

function ItemChip({ item, compact, colors }: { item: CalendarItem; compact?: boolean; colors?: DayColors }) {
  const kind = item.day_kind as DayKind | "";
  return (
    <button
      className={`cal-item layer-${item.layer} ${item.done ? "done" : ""} ${kind ? `day-${kind}` : ""}`}
      style={kind && colors ? { color: dayHex(colors[kind]) } : undefined}
      onClick={(e) => {
        e.stopPropagation();
        openItem(item);
      }}
      title={`${LAYER_LABEL[item.layer] ?? ""} · ${item.title}${item.source === "ai" ? " (AI가 기록에서 찾음)" : ""}`}
    >
      {!compact && item.time && <span className="cal-time">{fmtTime(item.time)}</span>}
      {item.kind === "task" && <span className="cal-kind">{item.done ? "✓" : "□"}</span>}
      {item.kind === "birthday" && <span className="cal-kind">🎂</span>}
      {item.kind === "tt_change" && <span className="cal-kind">⇄</span>}
      <span className="cal-title">{item.title}</span>
      {item.grades && <span className="cal-grade">{gradeTag(item.grades)}</span>}
      {item.source === "ai" && !compact && <span className="cal-ai">AI</span>}
    </button>
  );
}

export function CalendarPage() {
  const [view, setView] = useState<CalendarView>("week");
  const [anchor, setAnchor] = useState<Date>(new Date());
  const [schoolYear, setSchoolYear] = useState(true);
  const r = useMemo(() => range(view, anchor, schoolYear), [view, anchor, schoolYear]);
  const data = useLoad(() => api.calendar(view, isoDate(r.start), isoDate(r.end)), [view, isoDate(r.start), isoDate(r.end)]);
  const items = data.data ?? [];
  const today = todayISO();
  const { navigate } = useStore();
  const settings = useLoad(() => api.getSettings(), []);
  const colors = settings.data?.day_colors;
  const dayStyle = (day: string) => {
    const k = dayKindOn(items, day);
    return k && colors ? { color: dayHex(colors[k]) } : undefined;
  };
  openStudent = (id) => navigate("students", { studentId: id });
  openNote = (id) => navigate("notes", { noteId: id });
  const hasBirthdays = items.some((i) => i.kind === "birthday");
  const hasChanges = items.some((i) => i.kind === "tt_change");

  const move = (dir: number) => {
    if (view === "week") setAnchor(addDays(anchor, 7 * dir));
    else if (view === "month") setAnchor(addMonths(anchor, dir));
    else setAnchor(new Date(anchor.getFullYear() + dir, anchor.getMonth(), 1));
  };
  const add = (day: string) => openEditor({ kind: "event", date: day, layer: DEFAULT_LAYER[view] });

  const title =
    view === "week"
      ? `${r.start.getFullYear()}년 ${r.start.getMonth() + 1}월 ${r.start.getDate()}일 ~ ${r.end.getMonth() + 1}월 ${r.end.getDate()}일`
      : view === "month"
        ? `${anchor.getFullYear()}년 ${anchor.getMonth() + 1}월`
        : schoolYear
          ? `${schoolYearOf(anchor)}학년도 (${schoolYearOf(anchor)}.3.~${schoolYearOf(anchor) + 1}.2.)`
          : `${anchor.getFullYear()}년`;

  return (
    <div className="page calendar">
      <div className="page-head">
        <h1>일정표</h1>
        <div className="head-actions">
          <div className="seg">
            {(["week", "month", "year"] as CalendarView[]).map((v) => (
              <button key={v} className={view === v ? "on" : ""} onClick={() => setView(v)}>
                {v === "week" ? "주간" : v === "month" ? "월간" : "학사력"}
              </button>
            ))}
          </div>
          <button className="primary" onClick={() => add(isoDate(anchor))}>
            <IconPlus size={15} /> {LAYER_LABEL[DEFAULT_LAYER[view]]} 추가
          </button>
        </div>
      </div>
      <div className="cal-nav">
        <button className="icon-btn" onClick={() => move(-1)} aria-label="이전">
          <IconLeft />
        </button>
        <strong className="cal-range">{title}</strong>
        <button className="icon-btn" onClick={() => move(1)} aria-label="다음">
          <IconRight />
        </button>
        <button onClick={() => setAnchor(new Date())}>오늘</button>
        <label className="inline">
          기준 날짜
          <input type="date" value={isoDate(anchor)} onChange={(e) => e.target.value && setAnchor(parseDate(e.target.value))} />
        </label>
        {view === "year" && (
          <label className="check">
            <input type="checkbox" checked={schoolYear} onChange={(e) => setSchoolYear(e.target.checked)} /> 학년도(3월 시작)로 보기
          </label>
        )}
        <span className="grow" />
        <div className="legend">
          {view === "year" && colors
            ? (Object.keys(DAY_KIND_LABEL) as DayKind[]).map((k) => (
                <span key={k} className="legend-item day-kind" style={{ color: dayHex(colors[k]) }}>
                  ● {DAY_KIND_LABEL[k]}
                </span>
              ))
            : [...(view === "month" ? ["annual", "monthly", "note", "task", "duty"] : ["annual", "monthly", "weekly", "note", "task", "duty"]), ...(hasBirthdays ? ["birthday"] : []), ...(hasChanges ? ["tt_change"] : [])].map((l) => (
                <span key={l} className={`legend-item layer-${l}`} title={LAYER_HELP[l] ?? ""}>
                  {LAYER_LABEL[l]}
                </span>
              ))}
        </div>
      </div>
      {data.error && <ErrorBox error={data.error} onRetry={data.reload} />}

      {view === "week" && (
        <div className="week-grid">
          {Array.from({ length: 7 }, (_, i) => {
            const d = addDays(r.start, i);
            const day = isoDate(d);
            const list = itemsOn(items, day);
            return (
              <div key={day} className={`week-col ${day === today ? "is-today" : ""} ${i >= 5 ? "weekend" : ""}`} onDoubleClick={() => add(day)}>
                <div className="week-head">
                  <span className="wd">{WEEK_HEAD[i]}</span>
                  <span className="dn" style={dayStyle(day)}>
                    {d.getDate()}
                  </span>
                  <button className="icon-btn tiny" onClick={() => add(day)} aria-label={`${d.getMonth() + 1}월 ${d.getDate()}일 일정 추가`} title="일정 추가">
                    <IconPlus size={14} />
                  </button>
                </div>
                <div className="week-items">
                  {list.length === 0 ? <span className="muted small">—</span> : list.map((it) => <ItemChip key={`${it.kind}-${it.id}`} item={it} colors={colors} />)}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {view === "month" && (
        <div className="month-grid">
          {WEEK_HEAD.map((w, i) => (
            <div key={w} className={`month-wd ${i >= 5 ? "weekend" : ""}`}>
              {w}
            </div>
          ))}
          {Array.from({ length: 42 }, (_, i) => {
            const d = addDays(r.start, i);
            const day = isoDate(d);
            const list = itemsOn(items, day);
            const other = d.getMonth() !== anchor.getMonth();
            return (
              <div key={day} className={`month-cell ${other ? "other" : ""} ${day === today ? "is-today" : ""}`} onClick={() => add(day)} role="button" tabIndex={0} aria-label={`${d.getMonth() + 1}월 ${d.getDate()}일`}>
                <span className="dn" style={dayStyle(day)}>
                  {d.getDate()}
                </span>
                {list.slice(0, 4).map((it) => (
                  <ItemChip key={`${it.kind}-${it.id}`} item={it} compact colors={colors} />
                ))}
                {list.length > 4 && (
                  <button
                    className="more"
                    onClick={(e) => {
                      e.stopPropagation();
                      setAnchor(d);
                      setView("week");
                    }}
                  >
                    +{list.length - 4}개 더
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}

      {view === "year" && (
        <div className="year-grid">
          {Array.from({ length: 12 }, (_, idx) => {
            const first = new Date(r.start.getFullYear(), r.start.getMonth() + idx, 1);
            const m = first.getMonth();
            const s = addDays(first, -first.getDay());
            const lastDay = new Date(first.getFullYear(), m + 1, 0);
            const monthItems = items.filter((i) => !i.hidden && i.start_date <= isoDate(lastDay) && i.end_date >= isoDate(first));
            return (
              <div key={isoDate(first)} className="mini-month">
                <button
                  className="mini-title"
                  onClick={() => {
                    setAnchor(first);
                    setView("month");
                  }}
                >
                  {first.getFullYear() !== r.start.getFullYear() ? `${first.getFullYear()}년 ` : ""}
                  {m + 1}월
                </button>
                <div className="mini-grid">
                  {YEAR_HEAD.map((w, i) => (
                    <span key={w} className="mini-wd" style={i === 0 && colors ? { color: dayHex(colors.holiday) } : undefined}>
                      {w}
                    </span>
                  ))}
                  {Array.from({ length: 42 }, (_, i) => {
                    const d = addDays(s, i);
                    const day = isoDate(d);
                    if (d.getMonth() !== m) return <span key={day} className="mini-day blank" />;
                    const has = itemsOn(items, day).length > 0;
                    // 일요일은 따로 적힌 학사력이 없어도 휴일 색
                    const kind = dayKindOn(items, day) ?? (d.getDay() === 0 ? "holiday" : null);
                    return (
                      <button
                        key={day}
                        className={`mini-day ${has ? "has" : ""} ${day === today ? "is-today" : ""} ${kind ? `kind-${kind}` : ""}`}
                        style={kind && colors ? { color: dayHex(colors[kind]) } : undefined}
                        onClick={() => add(day)}
                        title={has ? itemsOn(items, day).map((x) => x.title).join(", ") : `${m + 1}월 ${d.getDate()}일 학사력 추가`}
                      >
                        {d.getDate()}
                      </button>
                    );
                  })}
                </div>
                <ul className="mini-list">
                  {monthItems.map((it) => (
                    <li key={it.id}>
                      <button className="link" onClick={() => openItem(it)} style={it.day_kind && colors ? { color: dayHex(colors[it.day_kind as DayKind]) } : undefined}>
                        {parseDate(it.start_date).getMonth() + 1 !== m + 1 ? `${parseDate(it.start_date).getMonth() + 1}/` : ""}
                        {parseDate(it.start_date).getDate()}일({weekdayKo(parseDate(it.start_date))})
                        {it.end_date !== it.start_date && `~${parseDate(it.end_date).getMonth() !== parseDate(it.start_date).getMonth() ? `${parseDate(it.end_date).getMonth() + 1}/` : ""}${parseDate(it.end_date).getDate()}일`} {it.title}
                        {it.grades && <span className="cal-grade">{gradeTag(it.grades)}</span>}
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
        </div>
      )}
      <p className="page-desc">
        반영 방향: 학사력은 월간·주간 표에도 보이고, 월간 계획은 주간 표에도 보입니다. 주간 계획과 기록에서 찾은 일정·할 일은 학사력에 올라가지 않습니다. 학교를 정하고 나이스 인증키를 넣으면 학사일정을 나이스에서 받아 오며, 직접 추가한 학사력은 다시 받아도 그대로 남습니다.
      </p>
    </div>
  );
}
