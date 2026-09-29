import { api, errorText } from "../api";
import { AgendaRow } from "../components/AgendaRow";
import { openEditor } from "../components/Editors";
import { Empty, ErrorBox } from "../components/Fields";
import { IconPlus, IconSpark } from "../components/Icons";
import { NoteCard } from "../components/NoteCard";
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

export function TodayPage() {
  const { navigate, toast, bump } = useStore();
  const today = todayISO();
  const ov = useLoad(() => api.today(), []);
  const notes = useLoad(() => api.listNotes({ date: today, limit: 50 }), [today]);
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

      <div className="today-grid">
        <section className="panel">
          <h2>
            할 일 <span className="count">{totalOpen}</span>
          </h2>
          {!o ? (
            <p className="muted">불러오는 중…</p>
          ) : totalOpen === 0 && o.undated.length === 0 ? (
            <Empty title="2주 안에 할 일이 없습니다.">위 입력창에 “금요일까지 보호자 연락”처럼 적으면 날짜가 붙은 할 일이 생깁니다.</Empty>
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

        <aside className="side-col">
          <section className="panel">
            <h2>오늘 일정</h2>
            {o && o.events_today.length === 0 && <p className="muted small">오늘 일정이 없습니다.</p>}
            <ul className="agenda compact">{o?.events_today.map((it) => <AgendaRow key={`e-${it.id}`} item={it} showDate={false} />)}</ul>
            {o && o.events_upcoming.length > 0 && (
              <>
                <h3>다가오는 일정</h3>
                <ul className="agenda compact">{o.events_upcoming.map((it) => <AgendaRow key={`e-${it.id}`} item={it} />)}</ul>
              </>
            )}
            <button className="link" onClick={() => navigate("calendar")}>
              일정표 열기 →
            </button>
          </section>

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
        </aside>
      </div>

      <section className="panel">
        <h2>
          오늘 쓴 기록 <span className="count">{notes.data?.length ?? 0}</span>
        </h2>
        {notes.data && notes.data.length === 0 && <Empty title="오늘 쓴 기록이 없습니다.">맨 위 입력창에 생각나는 대로 적어 보세요. 카테고리를 고를 필요가 없습니다.</Empty>}
        <div className="note-list">{notes.data?.map((n) => <NoteCard key={n.id} note={n} />)}</div>
      </section>
    </div>
  );
}
