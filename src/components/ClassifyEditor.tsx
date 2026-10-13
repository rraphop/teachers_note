import { useEffect, useMemo, useState } from "react";
import { api, errorText } from "../api";
import { fmtDateTime } from "../lib/format";
import { AREA_LABEL, AREA_ORDER, KIND_LABEL } from "../lib/labels";
import { useStore } from "../lib/store";
import type { Area, Duty, ItemKind, Note, Plan, PlanItem, PlanStudent, ReminderSpec, Student } from "../types";
import { DateFields, ReminderPicker } from "./Fields";
import { IconPlus, IconTrash } from "./Icons";
import { confirmDialog, Modal } from "./Modal";
import { TextLines } from "./TextLines";

const KINDS: ItemKind[] = ["student", "task", "event", "duty"];
const PROFILE_LABEL: Record<string, string> = {
  phone: "전화번호",
  guardian: "보호자 연락처",
  address: "주소",
  gender: "성별",
  birthday: "생일",
  student_no: "학생개인번호",
  prev_record: "이전학적",
  number: "번호",
  remark: "비고",
};

function blankItem(kind: ItemKind): PlanItem {
  return {
    kind,
    text: "",
    students: [],
    area: "none",
    duty_id: null,
    date: null,
    time: null,
    date_expr: null,
    approximate: false,
    remind: null,
    origin: "manual",
    done: false,
    edited: false,
  };
}

function studentLabel(s: PlanStudent): string {
  return s.class_label ? `${s.class_label} ${s.name}` : s.name;
}

function StudentChips({ value, onChange, roster }: { value: PlanStudent[]; onChange: (v: PlanStudent[]) => void; roster: Student[] }) {
  const [q, setQ] = useState("");
  const matches = useMemo(() => {
    const t = q.trim();
    if (!t) return [];
    return roster.filter((s) => s.name.includes(t) || `${s.class_label ?? ""} ${s.name}`.includes(t)).slice(0, 8);
  }, [q, roster]);
  const add = (s: PlanStudent) => {
    if (!value.some((v) => (v.student_id !== null && v.student_id === s.student_id) || (v.name === s.name && v.class_label === s.class_label))) {
      onChange([...value, s]);
    }
    setQ("");
  };
  const addTyped = () => {
    const t = q.trim();
    if (!t) return;
    const m = /^(\d)\s*-\s*(\d{1,2})\s+(.+)$/.exec(t);
    if (m) add({ student_id: null, name: (m[3] ?? "").trim(), class_label: `${m[1]}-${m[2]}`, number: null, candidates: [] });
    else if (matches.length === 1 && matches[0]) {
      const s = matches[0];
      add({ student_id: s.id, name: s.name, class_label: s.class_label, number: s.number, candidates: [] });
    } else add({ student_id: null, name: t, class_label: null, number: null, candidates: [] });
  };
  return (
    <div className="field">
      <span>학생</span>
      <div className="chip-input">
        {value.map((s, i) => (
          <span key={`${s.student_id ?? s.name}-${i}`} className={`chip student ${s.candidates.length > 1 ? "warn" : ""}`}>
            {studentLabel(s)}
            {s.candidates.length > 1 && " (동명이인 — 학생을 골라 주세요)"}
            {s.student_id === null && s.candidates.length === 0 && <em> 새 학생</em>}
            <button type="button" aria-label={`${s.name} 빼기`} onClick={() => onChange(value.filter((_, j) => j !== i))}>
              ×
            </button>
          </span>
        ))}
        <input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addTyped();
            }
          }}
          placeholder="이름 입력 (예: 홍길동 또는 2-1 홍길동)"
        />
      </div>
      {matches.length > 0 && (
        <div className="suggest">
          {matches.map((s) => (
            <button type="button" key={s.id} onClick={() => add({ student_id: s.id, name: s.name, class_label: s.class_label, number: s.number, candidates: [] })}>
              {s.class_label ?? "반 미지정"} {s.number ? `${s.number}번 ` : ""}
              {s.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function ItemEditor({
  item,
  onChange,
  onRemove,
  roster,
  duties,
  base,
  defRemind,
}: {
  item: PlanItem;
  onChange: (i: PlanItem) => void;
  onRemove: () => void;
  roster: Student[];
  duties: Duty[];
  base: string;
  defRemind: ReminderSpec;
}) {
  const set = (patch: Partial<PlanItem>) => onChange({ ...item, ...patch, edited: true });
  const [showRemind, setShowRemind] = useState(false);
  return (
    <div className={`plan-item kind-${item.kind}`}>
      <div className="row gap wrap">
        <label className="field">
          <span>종류</span>
          <select value={item.kind} onChange={(e) => set({ kind: e.target.value as ItemKind })}>
            {KINDS.map((k) => (
              <option key={k} value={k}>
                {KIND_LABEL[k]}
              </option>
            ))}
          </select>
        </label>
        {item.kind === "student" && (
          <label className="field">
            <span>학생부 자료 영역</span>
            <select value={item.area} onChange={(e) => set({ area: e.target.value as Area })}>
              {[...AREA_ORDER, "none" as Area].map((a) => (
                <option key={a} value={a}>
                  {AREA_LABEL[a]}
                </option>
              ))}
            </select>
          </label>
        )}
        {(item.kind === "task" || item.kind === "duty") && (
          <label className="field">
            <span>담당업무</span>
            <select value={item.duty_id ?? ""} onChange={(e) => set({ duty_id: e.target.value ? Number(e.target.value) : null })}>
              <option value="">{item.kind === "duty" ? "(업무를 골라 주세요)" : "(연결 안 함)"}</option>
              {duties.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.detail ? `${d.name} / ${d.detail}` : d.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <span className="grow" />
        <span className={`tag src-${item.origin}`}>{item.origin === "manual" ? "직접" : item.origin === "rule" ? "규칙" : "AI"}</span>
        <button type="button" className="icon-btn" onClick={onRemove} aria-label="항목 빼기" title="항목 빼기">
          <IconTrash size={16} />
        </button>
      </div>
      <label className="field">
        <span>{item.kind === "student" ? "관찰 내용" : item.kind === "duty" ? "업무 내용" : "내용"}</span>
        <textarea rows={2} value={item.text} onChange={(e) => set({ text: e.target.value })} />
      </label>
      {(item.kind === "student" || item.kind === "task") && <StudentChips value={item.students} onChange={(v) => set({ students: v })} roster={roster} />}
      {item.kind === "student" ? (
        <label className="field">
          <span>관찰 날짜</span>
          <input type="date" value={item.date ?? base} onChange={(e) => set({ date: e.target.value || null })} />
        </label>
      ) : (
        <DateFields
          label={item.kind === "task" ? "기한" : item.kind === "event" ? "일정 날짜" : "관련 날짜 (선택)"}
          date={item.date}
          time={item.time}
          expr={item.date_expr}
          base={base}
          onChange={(v) => set({ date: v.date, time: v.time, date_expr: v.expr, approximate: false })}
        />
      )}
      {(item.kind === "task" || item.kind === "duty") && (
        <label className="check">
          <input type="checkbox" checked={item.done} onChange={(e) => set({ done: e.target.checked })} /> 완료
        </label>
      )}
      {item.kind !== "student" && (
        <div>
          <button type="button" className="link" onClick={() => setShowRemind((s) => !s)}>
            알림 설정 {showRemind ? "접기" : "열기"} ({(item.remind ?? { ...defRemind, enabled: !!item.date && defRemind.enabled }).enabled ? "켜짐" : "꺼짐"})
          </button>
          {showRemind && (
            <ReminderPicker value={item.remind ?? { ...defRemind, enabled: !!item.date && defRemind.enabled }} onChange={(r) => set({ remind: r })} hasDate={!!item.date} />
          )}
        </div>
      )}
      {item.kind === "event" && !item.date && <p className="hint warn">날짜가 없으면 일정으로 저장되지 않습니다.</p>}
      {item.kind === "duty" && !item.duty_id && <p className="hint warn">담당업무를 고르지 않으면 '업무 미지정'으로 저장됩니다.</p>}
    </div>
  );
}

/** 기록 하나의 분류를 직접 하거나 AI 결과를 고치는 창 */
export function ClassifyEditor({ noteId, manual, onClose }: { noteId: number; manual: boolean; onClose: () => void }) {
  const { toast, bump } = useStore();
  const [note, setNote] = useState<Note | null>(null);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [roster, setRoster] = useState<Student[]>([]);
  const [duties, setDuties] = useState<Duty[]>([]);
  const [defRemind, setDefRemind] = useState<ReminderSpec>({ enabled: true, offsets: [1], time: "08:30", custom_at: null });
  const [busy, setBusy] = useState(false);
  const [fromRules, setFromRules] = useState(false);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const [d, r, du, s] = await Promise.all([api.noteDetail(noteId), api.listStudents(null, null), api.listDuties(), api.getSettings()]);
        if (!alive) return;
        setNote(d.note);
        setRoster(r);
        setDuties(du.filter((x) => x.active));
        setDefRemind(s.remind_default);
        if (d.plan.items.length === 0 && (d.plan.profile?.length ?? 0) === 0 && (d.plan.specials?.length ?? 0) === 0) {
          const sug = await api.suggestPlan(noteId);
          if (!alive) return;
          setPlan({ ...sug, summary: d.plan.summary || sug.summary });
          setFromRules(sug.items.length > 0);
        } else {
          setPlan(d.plan);
        }
      } catch (e) {
        toast(errorText(e), "error");
      }
    })();
    return () => {
      alive = false;
    };
  }, [noteId, toast]);

  const base = note?.created_at.slice(0, 10) ?? "";
  const loadRules = async () => {
    if (plan && plan.items.length > 0 && !(await confirmDialog("지금 목록을 규칙 기반 제안으로 바꿀까요?", { okText: "바꾸기" }))) return;
    try {
      const sug = await api.suggestPlan(noteId);
      setPlan((p) => ({ ...sug, summary: p?.summary || sug.summary }));
      setFromRules(true);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const save = async () => {
    if (!plan) return;
    setBusy(true);
    try {
      await api.savePlan(noteId, plan);
      toast("분류를 저장했습니다.");
      bump();
      onClose();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      wide
      title={manual ? "직접 분류" : "분류 수정"}
      onClose={onClose}
      footer={
        <>
          <button onClick={() => void loadRules()}>규칙으로 다시 제안</button>
          <span className="grow" />
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={() => void save()} disabled={busy || !plan}>
            저장
          </button>
        </>
      }
    >
      {!note || !plan ? (
        <p className="muted">불러오는 중…</p>
      ) : (
        <>
          <div className="raw-box">
            <div className="raw-head">원문 · {fmtDateTime(note.created_at)}</div>
            <p className="raw-text"><TextLines text={note.raw_text} /></p>
          </div>
          {fromRules && <p className="hint">AI 없이 규칙으로 찾은 제안을 채워 두었습니다. 확인하고 고친 뒤 저장하세요.</p>}
          {plan.guard_notes.length > 0 && (
            <details className="guard">
              <summary>안전 규칙이 AI 결과를 고친 내용 {plan.guard_notes.length}건</summary>
              <ul>
                {plan.guard_notes.map((g, i) => (
                  <li key={i}>{g}</li>
                ))}
              </ul>
            </details>
          )}
          <label className="field">
            <span>요약</span>
            <input value={plan.summary} onChange={(e) => setPlan({ ...plan, summary: e.target.value })} placeholder="한 줄 요약" />
          </label>
          {(plan.profile?.length ?? 0) > 0 && (
            <div className="plan-profile">
              <h4>학생 명렬표에 넣을 정보 (학생부 자료·할 일로 보내지 않음)</h4>
              <ul>
                {plan.profile!.map((p, i) => (
                  <li key={i}>
                    <b>{p.student.class_label ?? ""} {p.student.name}</b> · {PROFILE_LABEL[p.field] ?? p.field}
                    {p.relation ? ` (${p.relation})` : ""}: {p.value}
                    <button className="ghost danger" onClick={() => setPlan({ ...plan, profile: plan.profile!.filter((_, j) => j !== i) })} aria-label="이 정보 빼기">
                      빼기
                    </button>
                  </li>
                ))}
              </ul>
              <p className="small muted">이미 값이 있으면 바로 바꾸지 않고 기록 카드에서 ‘기존 → 새 값’을 확인합니다.</p>
            </div>
          )}
          {(plan.specials?.length ?? 0) > 0 && (
            <div className="plan-profile">
              <h4>어디에 둘지 물어볼 내용 (특이사항)</h4>
              <ul>
                {plan.specials!.map((sp, i) => (
                  <li key={i}>
                    <b>{sp.student.class_label ?? ""} {sp.student.name}</b> · “{sp.text}”
                    <button className="ghost danger" onClick={() => setPlan({ ...plan, specials: plan.specials!.filter((_, j) => j !== i) })} aria-label="이 질문 빼기">
                      빼기
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}
          <div className="plan-items">
            {plan.items.length === 0 && <p className="muted">정리할 항목이 없습니다. 아래 버튼으로 추가하거나 요약만 저장해도 됩니다.</p>}
            {plan.items.map((it, i) => (
              <ItemEditor
                key={i}
                item={it}
                roster={roster}
                duties={duties}
                base={base}
                defRemind={defRemind}
                onChange={(n) => setPlan({ ...plan, items: plan.items.map((x, j) => (j === i ? n : x)) })}
                onRemove={() => setPlan({ ...plan, items: plan.items.filter((_, j) => j !== i) })}
              />
            ))}
          </div>
          <div className="row gap wrap">
            {KINDS.map((k) => (
              <button key={k} type="button" onClick={() => setPlan({ ...plan, items: [...plan.items, { ...blankItem(k), date: k === "student" ? base : null }] })}>
                <IconPlus size={14} /> {KIND_LABEL[k]}
              </button>
            ))}
          </div>
        </>
      )}
    </Modal>
  );
}
