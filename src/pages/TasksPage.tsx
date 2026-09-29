import { useState } from "react";
import { api } from "../api";
import { AgendaRow } from "../components/AgendaRow";
import { openEditor } from "../components/Editors";
import { Empty, ErrorBox } from "../components/Fields";
import { IconPlus } from "../components/Icons";
import { addDays, isoDate, startOfWeek, todayISO } from "../lib/format";
import { useLoad } from "../lib/store";
import type { AgendaItem, Task } from "../types";

function toAgenda(t: Task): AgendaItem {
  return {
    kind: "task",
    id: t.id,
    title: t.title,
    date: t.due_date,
    end_date: null,
    time: t.due_time,
    date_expr: t.date_expr,
    approximate: t.approximate,
    done: t.done,
    remind_text: t.remind_text,
    remind_enabled: t.remind.enabled,
    next_reminder: t.next_reminder,
    source: t.source,
    note_id: t.note_id,
    duty_label: t.duty_label,
    layer: null,
  };
}

export function TasksPage() {
  const [tab, setTab] = useState<"open" | "done">("open");
  const tasks = useLoad(() => api.listTasks(tab), [tab]);
  const today = todayISO();
  const weekEnd = isoDate(addDays(startOfWeek(new Date()), 6));

  if (tasks.error) return <ErrorBox error={tasks.error} onRetry={tasks.reload} />;
  const list = tasks.data ?? [];
  const groups: Array<[string, Task[]]> =
    tab === "done"
      ? [["완료한 일", list]]
      : [
          ["기한 지남", list.filter((t) => t.due_date && t.due_date < today)],
          ["오늘", list.filter((t) => t.due_date === today)],
          ["이번 주", list.filter((t) => t.due_date && t.due_date > today && t.due_date <= weekEnd)],
          ["그 뒤", list.filter((t) => t.due_date && t.due_date > weekEnd)],
          ["날짜 없음", list.filter((t) => !t.due_date)],
        ];

  return (
    <div className="page">
      <div className="page-head">
        <h1>할 일</h1>
        <div className="head-actions">
          <div className="seg">
            <button className={tab === "open" ? "on" : ""} onClick={() => setTab("open")}>
              해야 할 일
            </button>
            <button className={tab === "done" ? "on" : ""} onClick={() => setTab("done")}>
              완료
            </button>
          </div>
          <button className="primary" onClick={() => openEditor({ kind: "task" })}>
            <IconPlus size={15} /> 할 일 추가
          </button>
        </div>
      </div>
      <p className="page-desc">기록에 “~까지”, “연락”, “제출”처럼 해야 할 행동이 적히면 자동으로 여기에 들어옵니다. 단순한 업무 사실은 할 일로 만들지 않습니다.</p>
      {list.length === 0 && !tasks.loading ? (
        <Empty title={tab === "open" ? "해야 할 일이 없습니다." : "완료한 일이 없습니다."} />
      ) : (
        groups
          .filter(([, g]) => g.length > 0)
          .map(([label, g]) => (
            <section key={label} className={`panel bucket ${label === "기한 지남" ? "overdue" : ""}`}>
              <h2>
                {label} <span className={`count ${label === "기한 지남" ? "danger" : ""}`}>{g.length}</span>
              </h2>
              <ul className="agenda">{g.map((t) => <AgendaRow key={t.id} item={toAgenda(t)} />)}</ul>
            </section>
          ))
      )}
    </div>
  );
}
