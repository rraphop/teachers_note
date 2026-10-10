import { useState } from "react";
import { api, errorText } from "../api";
import { fmtDateTime } from "../lib/format";
import { maskAddress, maskPhone, maskText, useMaskContacts } from "../lib/privacy";
import { useStore } from "../lib/store";
import type { Contact, Note, NoteQuestion, ProfileChange, Student } from "../types";
import { Modal } from "./Modal";

// 기록 속 학생 개인정보 → 학생 명렬표 (2차 수정목록 8번): 기록 카드의 태그·확인·질문, 보호자 연락처 편집

const SENSITIVE = new Set(["phone", "guardian", "address"]);

function shown(field: string, v: string, masked: boolean): string {
  if (!masked || !SENSITIVE.has(field)) return v;
  return field === "address" ? maskAddress(v) : maskPhone(v);
}

/** "학생 명렬표 · 주소" 태그 (넣은 것) */
export function ProfileChips({ note }: { note: Note }) {
  const masked = useMaskContacts();
  const applied = note.profile.filter((p) => p.status === "applied");
  const answered = note.questions.filter((q) => q.status !== "pending");
  if (applied.length === 0 && answered.length === 0) return null;
  return (
    <div className="chips">
      {applied.map((p) => (
        <span key={p.id} className="chip profile" title={`${p.student_label} · ${p.field_label}${p.relation ? ` (${p.relation})` : ""}: ${shown(p.field, p.new_value, masked)}`}>
          학생 명렬표 · {p.field_label}
          {p.relation ? ` (${p.relation})` : ""} <small>{p.student_label}</small>
        </span>
      ))}
      {answered.map((q) => (
        <AnsweredQuestion key={q.id} q={q} />
      ))}
    </div>
  );
}

function AnsweredQuestion({ q }: { q: NoteQuestion }) {
  const { toast, bump } = useStore();
  const other = q.status === "record" ? "roster" : "record";
  const move = async () => {
    try {
      await api.answerNoteQuestion(q.id, other);
      toast(other === "record" ? "학생부 자료로 옮겼습니다." : "학생 명렬표 특이사항으로 옮겼습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <span className="chip profile" title={q.text}>
      {q.status === "record" ? "학생부 자료" : "학생 명렬표 · 특이사항"} <small>{q.student_label}</small>
      <button className="chip-btn" onClick={() => void move()} title={other === "record" ? "학생부 자료로 옮기기" : "학생 명렬표 특이사항으로 옮기기"}>
        바꾸기
      </button>
    </span>
  );
}

/** 확인을 기다리는 개인정보: "기존 → 새 값" / 보호자 연락처 4개가 찼을 때 바꿀 번호 고르기 */
function ProfileConfirm({ c }: { c: ProfileChange }) {
  const { toast, bump } = useStore();
  const masked = useMaskContacts();
  const full = c.field === "guardian" && c.target_id === null && c.contacts.length > 0;
  const [pick, setPick] = useState<number | null>(full ? c.contacts[c.contacts.length - 1]!.id : null);
  const answer = async (accept: boolean) => {
    try {
      await api.resolveProfileChange(c.id, accept, accept && full ? pick : null);
      toast(accept ? `학생 명렬표의 ${c.field_label}을(를) 바꾸었습니다.` : "기존 값을 그대로 두었습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const rel = c.relation ? ` (${c.relation})` : "";
  return (
    <div className="profile-confirm" role="group" aria-label="학생 명렬표 확인">
      <b>
        학생 명렬표 확인 · {c.student_label} {c.field_label}
        {rel}
      </b>
      {full ? (
        <>
          <p className="small">보호자 연락처가 4개 찼습니다. 새 번호 {shown("guardian", c.new_value, masked)} 로 바꿀 번호를 고르세요.</p>
          <div className="contact-pick">
            {c.contacts.map((x) => (
              <label key={x.id} className="check">
                <input type="radio" name={`pick-${c.id}`} checked={pick === x.id} onChange={() => setPick(x.id)} /> {x.relation || "관계 없음"} {shown("guardian", x.phone, masked)}
              </label>
            ))}
          </div>
        </>
      ) : (
        <p className="small">
          기존: <span className="old">{shown(c.field, c.old_value, masked) || "(없음)"}</span> → 새 값: <b>{shown(c.field, c.new_value, masked)}</b>
          {c.field === "number" && !c.old_value && <> · 같은 학급에 이미 {c.new_value}번 학생이 있습니다.</>}
        </p>
      )}
      <div className="row gap">
        <button className="primary" onClick={() => void answer(true)} disabled={full && pick === null}>
          바꾸기
        </button>
        <button onClick={() => void answer(false)}>그대로 두기</button>
      </div>
    </div>
  );
}

/** "이 내용을 어디에 둘까요?" */
function QuestionBox({ q }: { q: NoteQuestion }) {
  const { toast, bump } = useStore();
  const answer = async (choice: "record" | "roster") => {
    try {
      await api.answerNoteQuestion(q.id, choice);
      toast(choice === "record" ? "학생부 자료 참고자료로 넣었습니다." : "학생 명렬표 특이사항에 넣었습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <div className="profile-confirm question" role="group" aria-label="특이사항 확인">
      <b>이 내용을 어디에 둘까요? · {q.student_label}</b>
      <p className="small">“{q.text}”</p>
      <div className="row gap">
        <button onClick={() => void answer("record")} title="학생부 자료의 참고자료로">
          학생부 자료
        </button>
        <button onClick={() => void answer("roster")} title="학생 명렬표 특이사항 칸으로 (학생부 자료·AI 초안에는 넣지 않음)">
          학생 명렬표 특이사항
        </button>
      </div>
    </div>
  );
}

/** 기록 카드에서 교사가 답해야 할 것 */
export function ProfileQuestions({ note }: { note: Note }) {
  const pending = note.profile.filter((p) => p.status === "pending");
  const questions = note.questions.filter((q) => q.status === "pending");
  if (pending.length === 0 && questions.length === 0) return null;
  return (
    <div className="profile-questions">
      {pending.map((c) => (
        <ProfileConfirm key={c.id} c={c} />
      ))}
      {questions.map((q) => (
        <QuestionBox key={q.id} q={q} />
      ))}
    </div>
  );
}

const RELATIONS = ["모", "부", "조모", "조부", "외조모", "외조부", "보호자", "이모", "고모", "삼촌", "외삼촌", "누나", "언니", "오빠", "집"];

/** 보호자 연락처 편집 (관계 + 번호, 최대 4개) */
export function ContactsDialog({ student, onClose }: { student: Student; onClose: () => void }) {
  const { toast, bump } = useStore();
  const [rows, setRows] = useState<Contact[]>(() => (student.contacts.length ? student.contacts.map((c) => ({ ...c })) : [{ id: 0, relation: "", phone: "" }]));
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    try {
      await api.setStudentContacts(student.id, rows.filter((r) => r.phone.trim()));
      toast("보호자 연락처를 저장했습니다.");
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
      title={`보호자 연락처 · ${student.class_label ?? ""} ${student.name}`}
      onClose={onClose}
      footer={
        <>
          <span className="grow" />
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={() => void save()} disabled={busy}>
            저장
          </button>
        </>
      }
    >
      <datalist id="relation-list">
        {RELATIONS.map((r) => (
          <option key={r} value={r} />
        ))}
      </datalist>
      <div className="contacts-edit">
        {rows.map((r, i) => (
          <div key={i} className="row gap">
            <span className="muted small">{i + 1}</span>
            <input list="relation-list" className="rel" value={r.relation} onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, relation: e.target.value } : x)))} placeholder="관계 (모·부·조모…)" aria-label={`연락처 ${i + 1} 관계`} />
            <input value={r.phone} onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, phone: e.target.value } : x)))} placeholder="010-0000-0000" aria-label={`연락처 ${i + 1} 번호`} />
            <button className="ghost danger" onClick={() => setRows(rows.filter((_, j) => j !== i))} aria-label={`연락처 ${i + 1} 지우기`}>
              지우기
            </button>
          </div>
        ))}
        {rows.length < 4 && (
          <button onClick={() => setRows([...rows, { id: 0, relation: "", phone: "" }])}>+ 연락처 추가</button>
        )}
        <p className="small muted">최대 4개. 기록에 “어머니 010-…”처럼 적으면 관계와 함께 자동으로 들어옵니다.</p>
      </div>
    </Modal>
  );
}

/** 명렬표 칸: 첫 연락처 + "+2", 누르면 모두 보기·고치기 */
export function ContactsCell({ student, masked }: { student: Student; masked: boolean }) {
  const [open, setOpen] = useState(false);
  const first = student.contacts[0];
  const text = (c: Contact) => `${c.relation ? `${c.relation} ` : ""}${masked ? maskPhone(c.phone) : c.phone}`;
  return (
    <td className="roster-cell contacts-cell" onClick={() => setOpen(true)} title={student.contacts.map(text).join("\n") || "눌러서 보호자 연락처 넣기"}>
      {first ? (
        <span>
          {text(first)}
          {student.contacts.length > 1 && <small className="more-count"> +{student.contacts.length - 1}</small>}
        </span>
      ) : (
        <span className="ph">010-0000-0000</span>
      )}
      {open && (
        <span onClick={(e) => e.stopPropagation()}>
          <ContactsDialog student={student} onClose={() => setOpen(false)} />
        </span>
      )}
    </td>
  );
}

const HISTORY_STATUS: Record<ProfileChange["status"], string> = { applied: "넣음", pending: "확인 기다림", rejected: "그대로 둠" };

/** 학생 상세: 기록에서 들어온 학생 명렬표 변경 이력 (최근 것부터) */
export function ProfileHistory({ list }: { list: ProfileChange[] }) {
  const { navigate } = useStore();
  const masked = useMaskContacts();
  if (list.length === 0) return null;
  // 보호자 연락처의 이전 값은 "모 010-…" 꼴이라 글 속 번호만 가린다
  const view = (field: string, v: string) => (masked && field === "guardian" ? maskText(v) : shown(field, v, masked));
  return (
    <section className="panel profile-history">
      <h2>
        명렬표 변경 이력 <span className="count">{list.length}</span>
      </h2>
      <ul className="plain-list">
        {list.map((c) => (
          <li key={c.id} className={`ph-${c.status}`}>
            <span className="muted small">{fmtDateTime(c.created_at)}</span> {c.field_label}
            {c.relation ? ` (${c.relation})` : ""}:{" "}
            {c.old_value && (
              <>
                <span className="old">{view(c.field, c.old_value)}</span> →{" "}
              </>
            )}
            <b>{view(c.field, c.new_value)}</b> <span className={`tag ${c.status === "pending" ? "warn" : ""}`}>{HISTORY_STATUS[c.status]}</span>
            {c.note_id !== null && (
              <button className="link" onClick={() => navigate("notes", { noteId: c.note_id ?? undefined })}>
                기록 보기
              </button>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
