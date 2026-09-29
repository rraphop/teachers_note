import { api, errorText } from "../api";
import { fmtDate, fmtDateTime, fmtTime, relDays, todayISO } from "../lib/format";
import { LAYER_LABEL, sourceLabel } from "../lib/labels";
import { useStore } from "../lib/store";
import type { AgendaItem } from "../types";
import { openEditor } from "./Editors";
import { IconBell, IconBellOff, IconEdit } from "./Icons";

/** 할 일·업무·일정 한 줄: 완료 체크, 날짜, 알림, 수정 */
export function AgendaRow({ item, showDate = true }: { item: AgendaItem; showDate?: boolean }) {
  const { toast, bump } = useStore();
  const today = todayISO();
  const overdue = !!item.date && item.date < today && !item.done && item.kind !== "event";
  const toggle = async (done: boolean) => {
    try {
      if (item.kind === "task") await api.setTaskDone(item.id, done);
      else if (item.kind === "duty_log") await api.setDutyLogDone(item.id, done);
      if (done) toast(`완료: ${item.title}`);
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const edit = () => openEditor(item.kind === "task" ? { kind: "task", id: item.id } : item.kind === "event" ? { kind: "event", id: item.id } : { kind: "duty_log", id: item.id });
  return (
    <li className={`agenda-row ${overdue ? "overdue" : ""} ${item.done ? "done" : ""}`}>
      {item.kind !== "event" ? (
        <input type="checkbox" className="big-check" checked={item.done} onChange={(e) => void toggle(e.target.checked)} aria-label={`${item.title} 완료`} />
      ) : (
        <span className={`layer-dot layer-${item.layer ?? "note"}`} title={LAYER_LABEL[item.layer ?? "note"]} />
      )}
      <button className="agenda-main" onClick={edit} title="눌러서 수정">
        <span className="agenda-title">{item.title}</span>
        <span className="agenda-meta">
          {item.kind === "duty_log" && <span className="tag duty">업무</span>}
          {item.kind === "event" && <span className="tag">{LAYER_LABEL[item.layer ?? "note"]}</span>}
          {item.duty_label && <span className="tag duty">{item.duty_label}</span>}
          {item.kind !== "event" && <span className={`tag src-${item.source}`}>{sourceLabel(item.source)}</span>}
          {item.date_expr && <span className="muted">“{item.date_expr}”</span>}
        </span>
      </button>
      {showDate && (
        <span className="agenda-when">
          {item.date ? (
            <>
              {fmtDate(item.date)}
              {item.time && ` ${fmtTime(item.time)}`}
              {item.approximate && " (대략)"}
              {item.kind !== "event" && <em className={overdue ? "late" : ""}>{relDays(item.date, today)}</em>}
            </>
          ) : (
            <span className="muted">날짜 없음</span>
          )}
        </span>
      )}
      <span className={`agenda-bell ${item.remind_enabled ? "" : "off"}`} title={item.next_reminder ? `다음 알림 ${fmtDateTime(item.next_reminder)}` : item.remind_text}>
        {item.remind_enabled ? <IconBell size={16} /> : <IconBellOff size={16} />}
        <span>{item.remind_enabled ? item.remind_text : "알림 없음"}</span>
      </span>
      <button className="icon-btn" onClick={edit} aria-label="수정" title="수정">
        <IconEdit size={16} />
      </button>
    </li>
  );
}
