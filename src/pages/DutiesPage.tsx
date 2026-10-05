import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { AgendaRow } from "../components/AgendaRow";
import { openEditor } from "../components/Editors";
import { Empty, ErrorBox } from "../components/Fields";
import { IconEdit, IconPlus } from "../components/Icons";
import { confirmDialog, Modal } from "../components/Modal";
import { fmtDate, fmtDateTime } from "../lib/format";
import { useLoad, useStore } from "../lib/store";
import type { Duty, DutyInput, DutyLog, Task } from "../types";

function DutyForm({ initial, onClose, onSaved }: { initial: Duty | null; onClose: () => void; onSaved: (id: number) => void }) {
  const { toast } = useStore();
  // 세부업무 하나로 통일 (예전 '분류 설명'·'핵심어'는 0.2 에서 세부업무로 합쳐졌다)
  const [f, setF] = useState<DutyInput>({
    name: initial?.name ?? "",
    detail: [initial?.detail, initial?.guide, initial?.keywords].filter((x) => x && x.trim()).join(", "),
    guide: "",
    keywords: "",
    active: initial?.active ?? true,
  });
  const save = async () => {
    try {
      if (initial) {
        await api.updateDuty(initial.id, f);
        onSaved(initial.id);
      } else {
        onSaved(await api.createDuty(f));
      }
      toast("업무를 저장했습니다.");
      onClose();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const del = async () => {
    if (!initial) return;
    if (!(await confirmDialog(`'${initial.name}' 업무를 지울까요? 업무 기록과 할 일은 남고 업무 연결만 풀립니다.`, { okText: "지우기", danger: true }))) return;
    await api.deleteDuty(initial.id).catch((e) => toast(errorText(e), "error"));
    onSaved(0);
    onClose();
  };
  return (
    <Modal
      title={initial ? "담당업무 수정" : "담당업무 등록"}
      onClose={onClose}
      footer={
        <>
          {initial && (
            <button className="ghost danger" onClick={() => void del()}>
              삭제
            </button>
          )}
          <span className="grow" />
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={() => void save()} disabled={!f.name.trim()}>
            저장
          </button>
        </>
      }
    >
      <label className="field">
        <span>업무명</span>
        <input value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} placeholder="예: 교무기획" />
      </label>
      <label className="field">
        <span>세부업무</span>
        <textarea
          rows={4}
          value={f.detail}
          onChange={(e) => setF({ ...f, detail: e.target.value })}
          placeholder={"낱말, 구, 문장 어느 것이든 적어 주세요.\n예: 학사일정 관리, 학사력, 수업일수\n교육청에 학사일정 변경을 보고하고 교직원에게 안내함"}
        />
        <small className="hint">
          AI가 이 내용을 읽고 기록이 이 업무에 해당하는지 판단합니다. 쉼표로 나눈 짧은 낱말(예: 학사력, 수업일수)은 AI가 없어도 분류에 쓰입니다.
        </small>
      </label>
      <label className="check">
        <input type="checkbox" checked={f.active} onChange={(e) => setF({ ...f, active: e.target.checked })} /> 분류에 사용 (올해 맡은 업무)
      </label>
    </Modal>
  );
}

function toAgendaTask(t: Task) {
  return {
    kind: "task" as const,
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
    duty_label: null,
    layer: null,
    memo: "",
    day_kind: "",
    day_color: null,
  };
}

function LogRow({ l }: { l: DutyLog }) {
  const { toast, bump, navigate } = useStore();
  const toggle = async (done: boolean) => {
    await api.setDutyLogDone(l.id, done).catch((e) => toast(errorText(e), "error"));
    bump();
  };
  return (
    <li className={`log-row ${l.done ? "done" : ""}`}>
      <input type="checkbox" className="big-check" checked={l.done} onChange={(e) => void toggle(e.target.checked)} aria-label="처리 완료" />
      <div className="grow">
        <p className="log-content">{l.content}</p>
        <div className="agenda-meta">
          <span className="muted">{fmtDateTime(l.created_at)}</span>
          {l.date && <span className="tag">{fmtDate(l.date)}</span>}
          {!l.duty_label && <span className="tag warn">업무 미지정</span>}
          <span className={`tag src-${l.source}`}>{l.source === "manual" ? "직접" : "AI"}</span>
          {l.remind.enabled && l.date && <span className="muted">🔔 {l.remind_text}</span>}
          {l.note_id && (
            <button className="link" onClick={() => navigate("notes", { noteId: l.note_id ?? undefined })}>
              원문
            </button>
          )}
        </div>
      </div>
      <button className="icon-btn" onClick={() => openEditor({ kind: "duty_log", id: l.id })} aria-label="수정" title="수정">
        <IconEdit size={16} />
      </button>
    </li>
  );
}

export function DutiesPage() {
  const { params, bump } = useStore();
  const [sel, setSel] = useState<number | null>(params.dutyId ?? null);
  const [form, setForm] = useState<Duty | null | "new">(null);
  const duties = useLoad(() => api.listDuties(), []);
  const board = useLoad(() => api.dutyBoard(sel), [sel]);
  useEffect(() => {
    if (params.dutyId) setSel(params.dutyId);
  }, [params.dutyId]);
  const current = duties.data?.find((d) => d.id === sel) ?? null;

  return (
    <div className="page">
      <div className="page-head">
        <h1>업무</h1>
        <div className="head-actions">
          <button onClick={() => openEditor({ kind: "duty_log", dutyId: sel })}>
            <IconPlus size={15} /> 업무 기록
          </button>
          <button className="primary" onClick={() => setForm("new")}>
            <IconPlus size={15} /> 담당업무 등록
          </button>
        </div>
      </div>
      <p className="page-desc">학교 업무분장표의 내 업무를 등록하면, 기록이 업무별로 자동으로 모입니다. 해야 할 행동이나 기한이 적힌 부분만 따로 할 일이 됩니다.</p>
      {duties.error && <ErrorBox error={duties.error} onRetry={duties.reload} />}
      {duties.data && duties.data.length === 0 && (
        <Empty title="등록한 담당업무가 없습니다.">
          예: 업무명 “교무기획”, 세부업무 “학사일정 관리, 학사력, 수업일수, 교육청에 학사일정 변경 보고”
          <div>
            <button className="primary" onClick={() => setForm("new")}>
              담당업무 등록하기
            </button>
          </div>
        </Empty>
      )}
      <div className="duty-layout">
        <aside className="duty-list">
          <button className={sel === null ? "on" : ""} onClick={() => setSel(null)}>
            전체 업무 기록
          </button>
          {duties.data?.map((d) => (
            <button key={d.id} className={`${sel === d.id ? "on" : ""} ${d.active ? "" : "inactive"}`} onClick={() => setSel(d.id)} title={d.active ? "" : "분류에 쓰지 않는 업무"}>
              <span className="duty-name">
                <b>{d.name}</b>
                {d.detail && <small>{d.detail.length > 24 ? `${d.detail.slice(0, 24)}…` : d.detail}</small>}
              </span>
              <span className="duty-counts">
                <span className="count" title="업무 기록 수">기록 {d.log_count}</span>
                {d.open_task_count > 0 && <span className="count warn">할 일 {d.open_task_count}</span>}
              </span>
            </button>
          ))}
        </aside>
        <div className="duty-main">
          {current && (
            <section className="panel duty-profile">
              <div className="row gap">
                <div className="grow">
                  <h2>{current.name}</h2>
                  {current.detail ? <p className="small pre-line">{current.detail}</p> : <p className="small muted">세부업무를 적으면 AI가 기록을 이 업무로 더 잘 분류합니다.</p>}
                </div>
                <button onClick={() => setForm(current)}>
                  <IconEdit size={15} /> 수정
                </button>
              </div>
            </section>
          )}
          <section className="panel">
            <h2>
              연결된 할 일 <span className="count">{board.data?.tasks.filter((t) => !t.done).length ?? 0}</span>
            </h2>
            {board.data && board.data.tasks.length === 0 && <p className="muted small">연결된 할 일이 없습니다.</p>}
            <ul className="agenda">{board.data?.tasks.map((t) => <AgendaRow key={t.id} item={toAgendaTask(t)} />)}</ul>
          </section>
          <section className="panel">
            <h2>
              업무 기록 <span className="count">{board.data?.logs.length ?? 0}</span>
            </h2>
            {board.data && board.data.logs.length === 0 && <p className="muted small">업무 기록이 없습니다.</p>}
            <ul className="log-list">{board.data?.logs.map((l) => <LogRow key={l.id} l={l} />)}</ul>
          </section>
        </div>
      </div>
      {form && (
        <DutyForm
          initial={form === "new" ? null : form}
          onClose={() => setForm(null)}
          onSaved={(id) => {
            if (id) setSel(id);
            else setSel(null);
            bump();
          }}
        />
      )}
    </div>
  );
}
