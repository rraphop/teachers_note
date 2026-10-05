import { useState } from "react";
import { api, errorText } from "../api";
import { fmtDateTime } from "../lib/format";
import { useLoad, useStore } from "../lib/store";
import type { AgendaItem, SummaryLine } from "../types";
import { AgendaRow } from "./AgendaRow";
import { openEditor } from "./Editors";
import { IconRefresh } from "./Icons";

/** 오늘 화면: 오늘 해야 할 업무 요약 (3~5줄, 시간순, 휴일·시험은 맨 위). 줄을 누르면 근거 일정·할 일·시간표로 간다. */
export function BriefingPanel({ upcoming, hasModel }: { upcoming: AgendaItem[]; hasModel: boolean }) {
  const { toast, navigate } = useStore();
  const b = useLoad(() => api.todayBriefing(), []);
  const [busy, setBusy] = useState(false);
  const refresh = async () => {
    setBusy(true);
    try {
      await api.refreshBriefing();
      b.reload();
      toast("AI가 오늘 업무를 다시 요약했습니다.");
    } catch (e) {
      toast(errorText(e), "error");
      b.reload();
    } finally {
      setBusy(false);
    }
  };
  const open = (l: SummaryLine) => {
    if (l.target_kind === "event" && l.target_id) openEditor({ kind: "event", id: l.target_id });
    else if (l.target_kind === "task" && l.target_id) openEditor({ kind: "task", id: l.target_id });
    else if (l.target_kind === "duty_log" && l.target_id) openEditor({ kind: "duty_log", id: l.target_id });
    else if (l.target_kind === "timetable") navigate("timetable");
    else navigate("tasks");
  };
  const d = b.data;
  return (
    <section className="panel briefing" id="today-briefing">
      <div className="section-head">
        <h2>오늘 업무 요약</h2>
        <button className="icon-btn" onClick={() => void refresh()} disabled={busy || !hasModel} title={hasModel ? "AI로 다시 요약" : "AI 모델을 설치하면 AI로 요약할 수 있습니다"} aria-label="새로고침">
          <IconRefresh size={16} />
        </button>
      </div>
      {b.error && <p className="small err">{b.error}</p>}
      {!d ? (
        <p className="muted small">불러오는 중…</p>
      ) : d.empty ? (
        <p className="muted small">오늘 챙길 일정·수업·할 일이 없습니다.</p>
      ) : (
        <ol className="brief-lines">
          {d.lines.map((l, i) => (
            <li key={i} className={l.top ? "top" : ""}>
              <button className="brief-line" onClick={() => open(l)} title="눌러서 근거 보기">
                {l.time && !l.text.startsWith(l.time) && <span className="brief-time">{l.time}</span>}
                <span>{l.text}</span>
              </button>
            </li>
          ))}
        </ol>
      )}
      {d && !d.empty && (
        <p className="small muted brief-by">
          {busy ? "AI가 요약하는 중…" : d.by === "ai" ? `AI 요약 · ${fmtDateTime(d.generated_at)}` : "일정·할 일·시간표에서 바로 모은 요약입니다. 새로고침을 누르면 AI가 다시 요약합니다."}
        </p>
      )}
      {upcoming.length > 0 && (
        <>
          <h3>다가오는 일정</h3>
          <ul className="agenda compact">{upcoming.map((it) => <AgendaRow key={`e-${it.id}`} item={it} />)}</ul>
        </>
      )}
      <button className="link" onClick={() => navigate("calendar")}>
        일정표 열기 →
      </button>
    </section>
  );
}
