import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { AgendaRow } from "../components/AgendaRow";
import { openEditor } from "../components/Editors";
import { Empty, ErrorBox } from "../components/Fields";
import { IconLayout, IconPlus, IconSpark } from "../components/Icons";
import { BriefingPanel } from "../components/BriefingPanel";
import { MealsPanel } from "../components/MealsPanel";
import { MemoPanel } from "../components/MemoPanel";
import { NoteCard } from "../components/NoteCard";
import { TodayBoard } from "../components/TodayBoard";
import { fmtDate, fmtDateLong, fmtDateTime, todayISO } from "../lib/format";
import { useLoad, useStore } from "../lib/store";
import type { AgendaItem } from "../types";

function groupByDate(items: AgendaItem[]): Array<[string, AgendaItem[]]> {
  const m = new Map<string, AgendaItem[]>();
  for (const it of items) {
    const k = it.date ?? "";
    m.set(k, [...(m.get(k) ?? []), it]);
  }
  return [...m.entries()];
}

function TodayTimetablePanel() {
  const { navigate } = useStore();
  const tt = useLoad(() => api.todayTimetable(), []);
  // 1분마다 '지금 교시' 표시를 새로 고친다
  const [, setTick] = useState(0);
  useEffect(() => {
    const t = window.setInterval(() => {
      setTick((x) => x + 1);
      tt.reload();
    }, 60000);
    return () => window.clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const d = tt.data;
  if (!d) return null;
  const weekend = d.slots.length === 0;
  return (
    <section className="panel today-tt">
      <h2>
        오늘 시간표 <span className="muted small">{d.school_year}학년도 {d.semester}학기</span>
      </h2>
      {!d.has_timetable ? (
        <p className="muted small">
          개인 시간표를 입력하면 오늘 수업이 여기에 보입니다.{" "}
          <button className="link" onClick={() => navigate("timetable")}>
            시간표 입력하기 →
          </button>
        </p>
      ) : weekend ? (
        <p className="muted small">오늘은 수업이 없는 날입니다.</p>
      ) : (
        <ul className="tt-today">
          {d.slots.map((s) =>
            s.period.kind === "lunch" ? (
              <li key="lunch" className={`lunch ${s.status}`}>
                <span className="tt-time">{s.period.start}</span>
                <span>점심</span>
              </li>
            ) : (
              <li key={s.period.no} className={`${s.status} ${s.cell || s.change ? "" : "free"} ${s.change ? "changed" : ""}`}>
                <span className="tt-time">{s.period.start}</span>
                <b className="tt-no">{s.period.label}</b>
                {s.cell ? (
                  <span className={s.change ? "was" : ""}>
                    {s.cell.class_label && <b>{s.cell.class_label} </b>}
                    {s.cell.subject}
                    {s.cell.room && <small className="muted"> · {s.cell.room}</small>}
                  </span>
                ) : (
                  !s.change && <span className="muted">공강</span>
                )}
                {s.change && (
                  <span className="tt-change" title={`기록: ${s.change.text}`}>
                    <span className="tag warn">{s.change.change_type || "변경"}</span> {[s.change.class_label, s.change.subject].filter(Boolean).join(" ")}
                    {s.change.room && <small className="muted"> · {s.change.room}</small>}
                  </span>
                )}
                {s.status === "now" && <span className="tag ok">지금</span>}
              </li>
            ),
          )}
        </ul>
      )}
    </section>
  );
}

export function TodayPage() {
  const { navigate, toast, bump } = useStore();
  const today = todayISO();
  const ov = useLoad(() => api.today(), []);
  // '오늘의 기록' 칸에서 다른 날짜의 기록도 찾아볼 수 있다
  const [noteDate, setNoteDate] = useState(today);
  const notes = useLoad(() => api.listNotes({ date: noteDate, limit: 200 }), [noteDate]);
  const isToday = noteDate === today;
  const shiftDate = (days: number) => {
    const d = new Date(`${noteDate}T00:00:00`);
    d.setDate(d.getDate() + days);
    const next = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    if (next <= today) setNoteDate(next);
  };
  const [editing, setEditing] = useState(false);
  const models = useLoad(() => api.modelCatalog(), []);
  const hasModel = !!models.data?.entries.some((e) => e.installed && e.selected);

  const reclassifyAll = async () => {
    try {
      const n = await api.reclassifyUnclassified();
      toast(`미분류 기록 ${n}건을 AI가 다시 정리합니다.`);
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  if (ov.error) return <ErrorBox error={ov.error} onRetry={ov.reload} />;
  const o = ov.data;
  const totalOpen = o ? o.overdue.length + o.due_today.length + o.upcoming.length : 0;
  const hasExtras = !!o && (o.birthdays_today.length > 0 || o.reminder_log.length > 0);

  return (
    <div className="page today">
      <div className="page-head">
        <h1>{fmtDateLong(today)}</h1>
        <div className="head-actions">
          <button onClick={() => openEditor({ kind: "task", date: today })}>
            <IconPlus size={15} /> 할 일
          </button>
          <button onClick={() => openEditor({ kind: "event", date: today, layer: "weekly" })}>
            <IconPlus size={15} /> 일정
          </button>
          <button className={editing ? "on" : ""} onClick={() => setEditing((v) => !v)} aria-pressed={editing} title="칸 위치를 바꾸거나 숨기기, 자동 배치로 되돌리기">
            <IconLayout size={15} /> 칸 배치
          </button>
        </div>
      </div>

      {models.data && !hasModel && (
        <div className="callout">
          <div>
            <strong>AI 모델이 아직 없습니다.</strong> 기록은 그대로 저장되고, 설치 후 한 번에 정리할 수 있습니다. 지금은 기록 카드의 ‘직접 분류’로도 정리할 수 있습니다.
          </div>
          <button className="primary" onClick={() => navigate("settings", { section: "models" })}>
            AI 모델 설치하기
          </button>
        </div>
      )}
      {o && o.unclassified_count > 0 && hasModel && (
        <div className="callout soft">
          <div>
            아직 정리되지 않은 기록이 <strong>{o.unclassified_count}건</strong> 있습니다.
          </div>
          <div className="row gap">
            <button onClick={() => navigate("notes", { section: "unclassified" })}>보기</button>
            <button className="primary" onClick={() => void reclassifyAll()}>
              <IconSpark size={15} /> 모두 AI로 분류
            </button>
          </div>
        </div>
      )}

      <TodayBoard
        editing={editing}
        onDone={() => setEditing(false)}
        panels={{
          tasks: (
            <section className="panel" id="today-tasks">
              <h2>
                할 일 <span className="count">{totalOpen}</span>
              </h2>
              {!o ? (
                <p className="muted">불러오는 중…</p>
              ) : totalOpen === 0 && o.undated.length === 0 ? (
                <Empty title="2주 안에 할 일이 없습니다.">아래 입력창에 “금요일까지 보호자 연락”처럼 적으면 날짜가 붙은 할 일이 생깁니다.</Empty>
              ) : (
                <>
                  {o.overdue.length > 0 && (
                    <div className="bucket overdue">
                      <h3>기한 지난 할 일 <span className="count danger">{o.overdue.length}</span></h3>
                      <ul className="agenda">{o.overdue.map((it) => <AgendaRow key={`${it.kind}-${it.id}`} item={it} />)}</ul>
                    </div>
                  )}
                  <div className="bucket">
                    <h3>오늘 할 일 <span className="count">{o.due_today.length}</span></h3>
                    {o.due_today.length === 0 ? <p className="muted small">오늘이 기한인 일은 없습니다.</p> : <ul className="agenda">{o.due_today.map((it) => <AgendaRow key={`${it.kind}-${it.id}`} item={it} />)}</ul>}
                  </div>
                  <div className="bucket">
                    <h3>앞으로 할 일 (2주) <span className="count">{o.upcoming.length}</span></h3>
                    {o.upcoming.length === 0 ? (
                      <p className="muted small">2주 안에 예정된 일이 없습니다.</p>
                    ) : (
                      groupByDate(o.upcoming).map(([d, items]) => (
                        <div key={d} className="date-group">
                          <div className="date-label">{fmtDate(d)}</div>
                          <ul className="agenda">{items.map((it) => <AgendaRow key={`${it.kind}-${it.id}`} item={it} showDate={false} />)}</ul>
                        </div>
                      ))
                    )}
                  </div>
                  {o.undated.length > 0 && (
                    <div className="bucket">
                      <h3>날짜 없는 할 일 <span className="count">{o.undated.length}</span></h3>
                      <ul className="agenda">{o.undated.map((it) => <AgendaRow key={`${it.kind}-${it.id}`} item={it} />)}</ul>
                    </div>
                  )}
                </>
              )}
            </section>
          ),
          briefing: <BriefingPanel upcoming={o?.events_upcoming ?? []} hasModel={hasModel} />,
          memo: <MemoPanel />,
          timetable: <TodayTimetablePanel />,
          meals: <MealsPanel />,
          extras: hasExtras ? (
            <>
              {o && o.birthdays_today.length > 0 && (
                <section className="panel birthday-panel">
                  <h2>🎂 오늘 생일</h2>
                  <p>{o.birthdays_today.join(", ")}</p>
                </section>
              )}
              {o && o.reminder_log.length > 0 && (
                <section className="panel">
                  <h2>최근 알림</h2>
                  <ul className="reminder-log">
                    {o.reminder_log.slice(0, 8).map((r) => (
                      <li key={r.id} className={`rl-${r.status}`}>
                        <span className="rl-status">{r.status === "fired" ? "보냄" : r.status === "missed" ? "놓침" : "실패"}</span>
                        <span className="grow">
                          [{r.label}] {r.title}
                          {r.error && <small className="err"> — {r.error}</small>}
                        </span>
                        <small className="muted">{fmtDateTime(r.fired_at)}</small>
                      </li>
                    ))}
                  </ul>
                </section>
              )}
            </>
          ) : editing ? (
            <section className="panel">
              <h2>생일·최근 알림</h2>
              <p className="muted small">오늘 생일인 학생이나 최근 알림이 있을 때만 보입니다.</p>
            </section>
          ) : null,
          notes: (
            <section className="panel" id="today-notes">
              <div className="section-head">
                <h2>
                  {isToday ? "오늘의 기록" : `${fmtDate(noteDate)}의 기록`} <span className="count">{notes.data?.length ?? 0}</span>
                </h2>
                <div className="date-finder" role="group" aria-label="날짜별 기록 찾기">
                  <button className="icon-btn" onClick={() => shiftDate(-1)} title="하루 전" aria-label="하루 전">
                    ‹
                  </button>
                  <input
                    type="date"
                    value={noteDate}
                    max={today}
                    onChange={(e) => e.target.value && setNoteDate(e.target.value)}
                    aria-label="기록을 찾을 날짜"
                    title="날짜를 골라 그날 쓴 기록 보기"
                  />
                  <button className="icon-btn" onClick={() => shiftDate(1)} disabled={isToday} title="하루 뒤" aria-label="하루 뒤">
                    ›
                  </button>
                  {!isToday && <button onClick={() => setNoteDate(today)}>오늘</button>}
                </div>
              </div>
              {notes.data && notes.data.length === 0 &&
                (isToday ? (
                  <Empty title="오늘 쓴 기록이 없습니다.">아래 입력창에 생각나는 대로 적어 보세요. 카테고리를 고를 필요가 없습니다.</Empty>
                ) : (
                  <Empty title={`${fmtDate(noteDate)}에 쓴 기록이 없습니다.`}>다른 날짜를 골라 보세요. 낱말로 찾으려면 왼쪽 ‘검색’을 쓰세요.</Empty>
                ))}
              <div className="note-list">{notes.data?.map((n) => <NoteCard key={n.id} note={n} />)}</div>
            </section>
          ),
        }}
      />
    </div>
  );
}
