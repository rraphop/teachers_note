import { useState } from "react";
import { api, errorText } from "../api";
import { fmtDate, fmtDateTime } from "../lib/format";
import { areaShort, STATUS_LABEL } from "../lib/labels";
import { useStore } from "../lib/store";
import type { Note } from "../types";
import { maskText, useMaskContacts } from "../lib/privacy";
import { ClassifyEditor } from "./ClassifyEditor";
import { ProfileChips, ProfileQuestions } from "./ProfileParts";
import { IconEdit, IconSpark, IconTrash } from "./Icons";
import { confirmDialog } from "./Modal";
import { Marked, TextLines } from "./TextLines";

export function StatusBadge({ status, by }: { status: Note["status"]; by?: string | null }) {
  // 기록 전체가 시간표 문장이라 규칙으로 바로 정리한 경우
  const label = status === "classified" && by === "rule" ? "시간표에 반영함" : STATUS_LABEL[status];
  return <span className={`status status-${status}`}>{status === "processing" && <span className="spinner" />}{label}</span>;
}

/** 다른 수업이 있던 시간표 칸: 바꾸기 / 그대로 두기 */
function ConflictChip({ id, label }: { id: number; label: string }) {
  const { toast, bump } = useStore();
  const resolve = async (replace: boolean) => {
    try {
      await api.resolveTtConflict(id, replace);
      toast(replace ? "시간표 칸을 바꾸었습니다." : "원래 수업을 그대로 두었습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <span className="chip tt_conflict">
      시간표 확인 · {label}
      <button className="chip-btn" onClick={() => void resolve(true)}>
        바꾸기
      </button>
      <button className="chip-btn" onClick={() => void resolve(false)}>
        그대로
      </button>
    </span>
  );
}

export function NoteChips({ note }: { note: Note }) {
  if (note.chips.length === 0) return null;
  return (
    <div className="chips">
      {note.chips.map((c, i) =>
        c.kind === "tt_conflict" && c.ref_id !== null ? (
          <ConflictChip key={i} id={c.ref_id} label={c.label} />
        ) : (
        <span key={i} className={`chip ${c.kind} ${c.done ? "done" : ""} ${c.area ? `area-${c.area}` : ""}`}>
          {c.kind === "student" && (
            <>
              <b>{c.label}</b> · {areaShort(c.area)}
            </>
          )}
          {c.kind === "task" && (
            <>
              할 일 · {c.label}
              {c.date && ` · ${fmtDate(c.date)}`}
            </>
          )}
          {c.kind === "event" && (
            <>
              일정 · {c.label}
              {c.date && ` · ${fmtDate(c.date)}`}
            </>
          )}
          {c.kind === "duty" && <>업무 · {c.label}</>}
          {c.kind === "timetable" && <>시간표 · {c.label}</>}
          {c.kind === "tt_change" && <>시간표 변경 · {c.label}</>}
        </span>
        ),
      )}
    </div>
  );
}

/** terms: 기록 화면 검색어 (찾은 곳을 표시하고, 긴 기록도 펼쳐 보여 준다) */
export function NoteCard({ note, highlight, terms }: { note: Note; highlight?: boolean; terms?: string[] }) {
  const { toast, bump, navigate } = useStore();
  const [editing, setEditing] = useState<null | "manual" | "edit">(null);
  const [rawEdit, setRawEdit] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const unclassified = note.status === "failed" || note.status === "pending";
  const searching = (terms?.length ?? 0) > 0;
  const long = note.raw_text.length > 220 && !searching;
  // 학생 명렬표에서 '연락처·주소 가리기'를 켜면 기록에서도 가린다
  const masked = useMaskContacts();

  const reclassify = async () => {
    try {
      const d = await api.noteDetail(note.id);
      if (d.has_user_changes && !(await confirmDialog("고치거나 완료한 항목이 있습니다. AI로 다시 분류하면 이 기록의 분류 결과가 새 결과로 바뀝니다. 계속할까요?", { okText: "다시 분류" }))) return;
      await api.reclassifyNote(note.id);
      toast("AI가 다시 정리합니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const moveTrack = async (track: "general" | "work") => {
    try {
      await api.setNoteTrack(note.id, track);
      toast(track === "work" ? "정리된 기록으로 옮겼습니다." : "기타 기록으로 되돌렸습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const remove = async () => {
    const n = note.chips.length;
    const msg = n > 0 ? `이 기록과 여기서 나온 항목 ${n}개(할 일·일정·관찰)를 함께 지웁니다. 지울까요?` : "이 기록을 지울까요?";
    if (!(await confirmDialog(msg, { okText: "지우기", danger: true }))) return;
    try {
      await api.deleteNote(note.id);
      toast("지웠습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const saveRaw = async () => {
    if (rawEdit === null) return;
    try {
      await api.updateNoteRaw(note.id, rawEdit);
      setRawEdit(null);
      bump();
      if (await confirmDialog("원문을 고쳤습니다. 분류도 AI로 다시 할까요? (아니오를 누르면 지금 분류를 그대로 둡니다)", { okText: "AI로 다시 분류" })) {
        await reclassify();
      }
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  return (
    <article className={`note-card ${highlight ? "highlight" : ""} ${unclassified ? "unclassified" : ""} ${note.is_work ? "track-work" : "track-general"}`} id={`note-${note.id}`}>
      <header className="note-head">
        <StatusBadge status={note.status} by={note.classified_by} />
        <time dateTime={note.created_at}>{fmtDateTime(note.created_at)}</time>
        {note.raw_edited_at && <span className="muted">원문 수정 {fmtDateTime(note.raw_edited_at)}</span>}
        <span className="grow" />
        {note.from_task_id !== null && (
          <span className="tag done-record" title="할 일을 완료로 체크해 남긴 기록입니다. 체크를 풀면 이 기록도 지워집니다.">
            할 일 완료
          </span>
        )}
        {note.ref_note_id !== null && (
          <button className="link" onClick={() => navigate("notes", { noteId: note.ref_note_id ?? undefined })} title="이 할 일이 나온 원래 기록으로">
            원래 기록
          </button>
        )}
        <span className={`track-tag ${note.is_work ? "work" : "general"}`} title={note.track ? "직접 옮긴 기록입니다. AI가 다시 분류해도 바뀌지 않습니다." : undefined}>
          {note.is_work ? "정리된 기록" : "기타 기록"}
          {note.track && " · 직접 옮김"}
        </span>
      </header>
      {note.summary && (
        <p className="note-summary">
          <Marked text={masked ? maskText(note.summary) : note.summary} terms={terms} />
        </p>
      )}
      {rawEdit === null ? (
        <p className={`note-raw ${long && !expanded ? "clamp" : ""}`}><TextLines text={masked ? maskText(note.raw_text) : note.raw_text} terms={terms} /></p>
      ) : (
        <div className="raw-edit">
          <textarea value={rawEdit} rows={Math.min(10, Math.max(3, rawEdit.split("\n").length + 1))} onChange={(e) => setRawEdit(e.target.value)} />
          <div className="row gap">
            <button onClick={() => setRawEdit(null)}>취소</button>
            <button className="primary" onClick={() => void saveRaw()} disabled={!rawEdit.trim()}>
              원문 저장
            </button>
          </div>
        </div>
      )}
      {long && rawEdit === null && (
        <button className="link" onClick={() => setExpanded((x) => !x)}>
          {expanded ? "접기" : "전체 보기"}
        </button>
      )}
      <NoteChips note={note} />
      <ProfileChips note={note} />
      <ProfileQuestions note={note} />
      {unclassified && note.status_reason && <p className="reason">{note.status_reason}</p>}
      {note.status === "processing" && <p className="reason soft">AI가 정리하고 있습니다. 기다리는 동안 다른 기록을 계속 써도 됩니다.</p>}
      {note.unknown_names.length > 0 && (
        <p className="reason soft">
          확인이 필요한 이름: {note.unknown_names.join(", ")} —{" "}
          <button className="link" onClick={() => setEditing("edit")}>
            분류 수정에서 학생 지정
          </button>{" "}
          또는{" "}
          <button className="link" onClick={() => navigate("students")}>
            학생 등록
          </button>
        </p>
      )}
      {note.guard_notes.length > 0 && (
        <details className="guard">
          <summary>안전 규칙이 AI 결과를 고친 내용 {note.guard_notes.length}건</summary>
          <ul>
            {note.guard_notes.map((g, i) => (
              <li key={i}>{g}</li>
            ))}
          </ul>
        </details>
      )}
      <footer className="note-actions">
        {unclassified ? (
          <>
            <button className="primary" onClick={() => void reclassify()}>
              <IconSpark size={15} /> AI로 다시 분류
            </button>
            <button onClick={() => setEditing("manual")}>직접 분류</button>
          </>
        ) : (
          <>
            <button onClick={() => setEditing("edit")} disabled={note.status === "processing"}>
              <IconEdit size={15} /> 분류 수정
            </button>
            <button onClick={() => void reclassify()} disabled={note.status === "processing"}>
              <IconSpark size={15} /> AI로 다시 분류
            </button>
          </>
        )}
        <span className="grow" />
        {note.is_work ? (
          <button className="ghost" onClick={() => void moveTrack("general")}>
            기타 기록으로 되돌리기
          </button>
        ) : (
          <button className="ghost" onClick={() => void moveTrack("work")} disabled={note.status === "processing"}>
            정리된 기록으로 보내기
          </button>
        )}
        <button className="ghost" onClick={() => setRawEdit(note.raw_text)} disabled={rawEdit !== null}>
          원문 수정
        </button>
        <button className="ghost danger" onClick={() => void remove()} aria-label="기록 삭제">
          <IconTrash size={15} />
        </button>
      </footer>
      {editing && <ClassifyEditor noteId={note.id} manual={editing === "manual"} onClose={() => setEditing(null)} />}
    </article>
  );
}
