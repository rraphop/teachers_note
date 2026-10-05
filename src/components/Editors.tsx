import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { fmtDateTime, todayISO } from "../lib/format";
import { LAYER_HELP, LAYER_LABEL } from "../lib/labels";
import { useStore } from "../lib/store";
import type { Duty, Layer, ReminderSpec } from "../types";
import { DateFields, ReminderPicker } from "./Fields";
import { confirmDialog, Modal } from "./Modal";

export type EditorReq =
  | { kind: "task"; id?: number; date?: string | null; dutyId?: number | null }
  | { kind: "event"; id?: number; date?: string; layer?: Layer }
  | { kind: "duty_log"; id?: number; dutyId?: number | null; date?: string | null };

let pushEditor: ((r: EditorReq) => void) | null = null;

/** 어느 화면에서든 할 일·일정·업무 기록 편집 창을 연다 */
export function openEditor(r: EditorReq) {
  pushEditor?.(r);
}

export function EditorHost() {
  const [req, setReq] = useState<EditorReq | null>(null);
  useEffect(() => {
    pushEditor = setReq;
    return () => {
      pushEditor = null;
    };
  }, []);
  if (!req) return null;
  const close = () => setReq(null);
  if (req.kind === "task") return <TaskEditor req={req} onClose={close} />;
  if (req.kind === "event") return <EventEditor req={req} onClose={close} />;
  return <DutyLogEditor req={req} onClose={close} />;
}

function useDuties(): Duty[] {
  const [d, setD] = useState<Duty[]>([]);
  useEffect(() => {
    api.listDuties().then(setD).catch(() => setD([]));
  }, []);
  return d;
}

function useDefaultRemind(): ReminderSpec | null {
  const [r, setR] = useState<ReminderSpec | null>(null);
  useEffect(() => {
    api
      .getSettings()
      .then((s) => setR(s.remind_default))
      .catch(() => setR({ enabled: true, offsets: [1], time: "08:30", custom_at: null }));
  }, []);
  return r;
}

function DutySelect({ value, onChange, duties }: { value: number | null; onChange: (v: number | null) => void; duties: Duty[] }) {
  return (
    <label className="field">
      <span>담당업무</span>
      <select value={value ?? ""} onChange={(e) => onChange(e.target.value ? Number(e.target.value) : null)}>
        <option value="">(연결 안 함)</option>
        {duties.map((d) => (
          <option key={d.id} value={d.id}>
            {d.detail ? `${d.name} / ${d.detail}` : d.name}
          </option>
        ))}
      </select>
    </label>
  );
}

function SourceLine({ source, edited, noteId, createdAt, onClose }: { source: string; edited: boolean; noteId: number | null; createdAt: string; onClose: () => void }) {
  const { navigate } = useStore();
  return (
    <p className="hint">
      {source === "manual" ? "직접 만든 항목" : edited ? "AI가 만들고 교사가 고친 항목" : "AI가 기록에서 찾은 항목"} · {fmtDateTime(createdAt)}
      {noteId !== null && (
        <>
          {" · "}
          <button
            type="button"
            className="link"
            onClick={() => {
              onClose();
              navigate("notes", { noteId });
            }}
          >
            원문 기록 보기
          </button>
        </>
      )}
    </p>
  );
}

function TaskEditor({ req, onClose }: { req: Extract<EditorReq, { kind: "task" }>; onClose: () => void }) {
  const { toast } = useStore();
  const duties = useDuties();
  const defRemind = useDefaultRemind();
  const [title, setTitle] = useState("");
  const [date, setDate] = useState<string | null>(req.date ?? null);
  const [time, setTime] = useState<string | null>(null);
  const [expr, setExpr] = useState<string | null>(null);
  const [dutyId, setDutyId] = useState<number | null>(req.dutyId ?? null);
  const [done, setDone] = useState(false);
  const [remind, setRemind] = useState<ReminderSpec | null>(null);
  const [meta, setMeta] = useState<{ source: string; edited: boolean; noteId: number | null; createdAt: string; next: string | null } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (req.id === undefined) return;
    api
      .getTask(req.id)
      .then((t) => {
        setTitle(t.title);
        setDate(t.due_date);
        setTime(t.due_time);
        setExpr(t.date_expr);
        setDutyId(t.duty_id);
        setDone(t.done);
        setRemind(t.remind);
        setMeta({ source: t.source, edited: t.edited, noteId: t.note_id, createdAt: t.created_at, next: t.next_reminder });
      })
      .catch((e) => toast(errorText(e), "error"));
  }, [req.id, toast]);

  const r = remind ?? defRemind;
  const save = async () => {
    setBusy(true);
    try {
      const input = { title, due_date: date, due_time: time, date_expr: expr, duty_id: dutyId, done, remind: r ?? null };
      if (req.id === undefined) await api.createTask(input);
      else await api.updateTask(req.id, input);
      toast("할 일을 저장했습니다.");
      onClose();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const del = async () => {
    if (req.id === undefined) return;
    if (!(await confirmDialog("이 할 일을 지울까요? 남은 알림도 함께 지워집니다.", { okText: "지우기", danger: true }))) return;
    await api.deleteTask(req.id).catch((e) => toast(errorText(e), "error"));
    toast("지웠습니다.");
    onClose();
  };
  return (
    <Modal
      title={req.id === undefined ? "할 일 추가" : "할 일 수정"}
      onClose={onClose}
      footer={
        <>
          {req.id !== undefined && (
            <button className="danger ghost" onClick={() => void del()}>
              삭제
            </button>
          )}
          <span className="grow" />
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={() => void save()} disabled={busy || !title.trim()}>
            저장
          </button>
        </>
      }
    >
      {meta && <SourceLine source={meta.source} edited={meta.edited} noteId={meta.noteId} createdAt={meta.createdAt} onClose={onClose} />}
      <label className="field">
        <span>내용</span>
        <textarea rows={2} value={title} onChange={(e) => setTitle(e.target.value)} placeholder="예: 보호자 연락" />
      </label>
      <DateFields
        label="기한"
        date={date}
        time={time}
        expr={expr}
        base={meta?.createdAt.slice(0, 10) ?? todayISO()}
        onChange={(v) => {
          setDate(v.date);
          setTime(v.time);
          setExpr(v.expr);
        }}
      />
      <div className="row gap">
        <DutySelect value={dutyId} onChange={setDutyId} duties={duties} />
        <label className="check self-end">
          <input type="checkbox" checked={done} onChange={(e) => setDone(e.target.checked)} /> 완료
        </label>
      </div>
      {r && <ReminderPicker value={r} onChange={setRemind} hasDate={!!date} />}
      {meta?.next && <p className="hint">다음 알림: {fmtDateTime(meta.next)}</p>}
    </Modal>
  );
}

function EventEditor({ req, onClose }: { req: Extract<EditorReq, { kind: "event" }>; onClose: () => void }) {
  const { toast } = useStore();
  const defRemind = useDefaultRemind();
  const [layer, setLayer] = useState<Layer>(req.layer ?? "weekly");
  const [title, setTitle] = useState("");
  const [start, setStart] = useState<string>(req.date ?? todayISO());
  const [end, setEnd] = useState<string>(req.date ?? todayISO());
  const [time, setTime] = useState<string | null>(null);
  const [expr, setExpr] = useState<string | null>(null);
  const [memo, setMemo] = useState("");
  const [remind, setRemind] = useState<ReminderSpec | null>(null);
  const [meta, setMeta] = useState<{ source: string; edited: boolean; noteId: number | null; createdAt: string; next: string | null } | null>(null);
  const [busy, setBusy] = useState(false);
  // 학사력은 대부분 하루짜리라 기본은 하루, 필요할 때만 기간으로 늘린다
  const [multiDay, setMultiDay] = useState(false);
  const showTime = layer !== "annual";

  useEffect(() => {
    if (req.id === undefined) return;
    api
      .getEvent(req.id)
      .then((e) => {
        setLayer(e.layer);
        setTitle(e.title);
        setStart(e.start_date);
        setEnd(e.end_date);
        setMultiDay(e.end_date > e.start_date);
        setTime(e.time);
        setExpr(e.date_expr);
        setMemo(e.memo);
        setRemind(e.remind);
        setMeta({ source: e.source, edited: e.edited, noteId: e.note_id, createdAt: e.created_at, next: e.next_reminder });
      })
      .catch((e) => toast(errorText(e), "error"));
  }, [req.id, toast]);

  const r = remind ?? (defRemind ? { ...defRemind, enabled: false } : null);
  const save = async () => {
    setBusy(true);
    try {
      const input = {
        layer,
        title,
        start_date: start,
        end_date: multiDay && end > start ? end : start,
        time: showTime ? time : null,
        date_expr: expr,
        memo,
        remind: r ?? null,
      };
      if (req.id === undefined) await api.createEvent(input);
      else await api.updateEvent(req.id, input);
      toast("일정을 저장했습니다.");
      onClose();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const del = async () => {
    if (req.id === undefined) return;
    if (!(await confirmDialog("이 일정을 지울까요?", { okText: "지우기", danger: true }))) return;
    await api.deleteEvent(req.id).catch((e) => toast(errorText(e), "error"));
    toast("지웠습니다.");
    onClose();
  };
  return (
    <Modal
      title={`${layer === "note" ? "일정" : LAYER_LABEL[layer]} ${req.id === undefined ? "추가" : "수정"}`}
      onClose={onClose}
      footer={
        <>
          {req.id !== undefined && (
            <button className="danger ghost" onClick={() => void del()}>
              삭제
            </button>
          )}
          <span className="grow" />
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={() => void save()} disabled={busy || !title.trim() || !start}>
            저장
          </button>
        </>
      }
    >
      {meta && <SourceLine source={meta.source} edited={meta.edited} noteId={meta.noteId} createdAt={meta.createdAt} onClose={onClose} />}
      <label className="field">
        <span>일정 종류</span>
        <select value={layer} onChange={(e) => setLayer(e.target.value as Layer)}>
          {(["annual", "monthly", "weekly", "note"] as Layer[]).map((l) => (
            <option key={l} value={l}>
              {LAYER_LABEL[l]}
            </option>
          ))}
        </select>
        <small className="hint">{LAYER_HELP[layer]}</small>
      </label>
      <label className="field">
        <span>내용</span>
        <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="예: 2학기 개학식" />
      </label>
      <div className="row gap wrap">
        <label className="field grow">
          <span>{multiDay ? "시작 날짜" : "날짜"}</span>
          <input
            type="date"
            value={start}
            onChange={(e) => {
              setStart(e.target.value);
              if (end < e.target.value) setEnd(e.target.value);
            }}
          />
        </label>
        {multiDay && (
          <label className="field grow">
            <span>끝 날짜</span>
            <input type="date" value={end} min={start} onChange={(e) => setEnd(e.target.value)} />
          </label>
        )}
        {showTime && (
          <label className="field">
            <span>시각 (선택)</span>
            <input type="time" value={time ?? ""} onChange={(e) => setTime(e.target.value || null)} />
          </label>
        )}
      </div>
      <label className="check">
        <input
          type="checkbox"
          checked={multiDay}
          onChange={(e) => {
            setMultiDay(e.target.checked);
            if (e.target.checked && end <= start) {
              const d = new Date(`${start}T00:00:00`);
              d.setDate(d.getDate() + 4);
              setEnd(`${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`);
            }
          }}
        />
        기간으로 설정 (여러 날 — 예: 시험 기간, 수련회, 방학)
      </label>
      {multiDay && end > start && (
        <p className="hint">
          {Math.round((new Date(`${end}T00:00:00`).getTime() - new Date(`${start}T00:00:00`).getTime()) / 86400000) + 1}일 동안
        </p>
      )}
      {expr && <p className="hint">원래 날짜 표현: {expr}</p>}
      <label className="field">
        <span>메모 (선택)</span>
        <textarea rows={2} value={memo} onChange={(e) => setMemo(e.target.value)} />
      </label>
      {r && <ReminderPicker value={r} onChange={setRemind} hasDate />}
      {meta?.next && <p className="hint">다음 알림: {fmtDateTime(meta.next)}</p>}
    </Modal>
  );
}

function DutyLogEditor({ req, onClose }: { req: Extract<EditorReq, { kind: "duty_log" }>; onClose: () => void }) {
  const { toast } = useStore();
  const duties = useDuties();
  const defRemind = useDefaultRemind();
  const [dutyId, setDutyId] = useState<number | null>(req.dutyId ?? null);
  const [content, setContent] = useState("");
  const [date, setDate] = useState<string | null>(req.date ?? null);
  const [time, setTime] = useState<string | null>(null);
  const [expr, setExpr] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const [remind, setRemind] = useState<ReminderSpec | null>(null);
  const [meta, setMeta] = useState<{ source: string; edited: boolean; noteId: number | null; createdAt: string; next: string | null } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (req.id === undefined) return;
    api
      .getDutyLog(req.id)
      .then((d) => {
        setDutyId(d.duty_id);
        setContent(d.content);
        setDate(d.date);
        setTime(d.time);
        setExpr(d.date_expr);
        setDone(d.done);
        setRemind(d.remind);
        setMeta({ source: d.source, edited: d.edited, noteId: d.note_id, createdAt: d.created_at, next: d.next_reminder });
      })
      .catch((e) => toast(errorText(e), "error"));
  }, [req.id, toast]);

  const r = remind ?? defRemind;
  const save = async () => {
    setBusy(true);
    try {
      const input = { duty_id: dutyId, content, date, time, date_expr: expr, done, remind: r ?? null };
      if (req.id === undefined) await api.createDutyLog(input);
      else await api.updateDutyLog(req.id, input);
      toast("업무 기록을 저장했습니다.");
      onClose();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const del = async () => {
    if (req.id === undefined) return;
    if (!(await confirmDialog("이 업무 기록을 지울까요?", { okText: "지우기", danger: true }))) return;
    await api.deleteDutyLog(req.id).catch((e) => toast(errorText(e), "error"));
    toast("지웠습니다.");
    onClose();
  };
  return (
    <Modal
      title={req.id === undefined ? "업무 기록 추가" : "업무 기록 수정"}
      onClose={onClose}
      footer={
        <>
          {req.id !== undefined && (
            <button className="danger ghost" onClick={() => void del()}>
              삭제
            </button>
          )}
          <span className="grow" />
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={() => void save()} disabled={busy || !content.trim()}>
            저장
          </button>
        </>
      }
    >
      {meta && <SourceLine source={meta.source} edited={meta.edited} noteId={meta.noteId} createdAt={meta.createdAt} onClose={onClose} />}
      <DutySelect value={dutyId} onChange={setDutyId} duties={duties} />
      <label className="field">
        <span>내용</span>
        <textarea rows={3} value={content} onChange={(e) => setContent(e.target.value)} />
      </label>
      <DateFields
        label="관련 날짜 (선택)"
        date={date}
        time={time}
        expr={expr}
        base={meta?.createdAt.slice(0, 10) ?? todayISO()}
        onChange={(v) => {
          setDate(v.date);
          setTime(v.time);
          setExpr(v.expr);
        }}
      />
      <label className="check">
        <input type="checkbox" checked={done} onChange={(e) => setDone(e.target.checked)} /> 처리 완료
      </label>
      {r && <ReminderPicker value={r} onChange={setRemind} hasDate={!!date} />}
      {meta?.next && <p className="hint">다음 알림: {fmtDateTime(meta.next)}</p>}
    </Modal>
  );
}
