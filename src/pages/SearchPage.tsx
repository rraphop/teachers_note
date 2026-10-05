import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { openEditor } from "../components/Editors";
import { Empty, ErrorBox } from "../components/Fields";
import { NoteChips, StatusBadge } from "../components/NoteCard";
import { fmtDate, fmtDateTime } from "../lib/format";
import { LAYER_LABEL } from "../lib/labels";
import { useLoad, useStore } from "../lib/store";
import type { SearchQuery } from "../types";

function Mark({ text, terms }: { text: string; terms: string[] }) {
  const t = terms.filter((x) => x.length > 0).map((x) => x.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (t.length === 0) return <>{text}</>;
  const re = new RegExp(`(${t.join("|")})`, "gi");
  return (
    <>
      {text.split(re).map((p, i) =>
        terms.some((x) => x.toLowerCase() === p.toLowerCase()) ? <mark key={i}>{p}</mark> : <span key={i}>{p}</span>,
      )}
    </>
  );
}

export function SearchPage() {
  const { params, navigate } = useStore();
  const [text, setText] = useState(params.query ?? "");
  const [q, setQ] = useState<SearchQuery>({ text: params.query ?? "" });
  const [studentId, setStudentId] = useState<string>("");
  const [dutyId, setDutyId] = useState<string>("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [status, setStatus] = useState("all");
  const ref = useRef<HTMLInputElement>(null);
  const students = useLoad(() => api.listStudents(null, null), []);
  const duties = useLoad(() => api.listDuties(), []);

  useEffect(() => {
    ref.current?.focus();
  }, []);
  useEffect(() => {
    const t = window.setTimeout(() => {
      setQ({
        text,
        student_id: studentId ? Number(studentId) : null,
        duty_id: dutyId ? Number(dutyId) : null,
        from: from || null,
        to: to || null,
        status,
        limit: 200,
      });
    }, 250);
    return () => window.clearTimeout(t);
  }, [text, studentId, dutyId, from, to, status]);

  const res = useLoad(() => api.search(q), [JSON.stringify(q)]);
  const r = res.data;
  const empty = !text.trim() && !studentId && !dutyId && !from && !to;

  return (
    <div className="page">
      <div className="page-head">
        <h1>검색</h1>
      </div>
      <div className="search-bar">
        <input ref={ref} className="search-big" value={text} onChange={(e) => setText(e.target.value)} placeholder="찾을 낱말 (예: 홍길동, 지각, 공개수업)" />
      </div>
      <div className="filters row gap wrap">
        <label className="inline">
          학생
          <select value={studentId} onChange={(e) => setStudentId(e.target.value)}>
            <option value="">전체</option>
            {students.data?.map((s) => (
              <option key={s.id} value={s.id}>
                {s.class_label ?? ""} {s.name}
              </option>
            ))}
          </select>
        </label>
        <label className="inline">
          업무
          <select value={dutyId} onChange={(e) => setDutyId(e.target.value)}>
            <option value="">전체</option>
            {duties.data?.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
              </option>
            ))}
          </select>
        </label>
        <label className="inline">
          기간
          <input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />~
          <input type="date" value={to} onChange={(e) => setTo(e.target.value)} />
        </label>
        <label className="inline">
          상태
          <select value={status} onChange={(e) => setStatus(e.target.value)}>
            <option value="all">전체</option>
            <option value="classified">정리됨</option>
            <option value="unclassified">미분류</option>
          </select>
        </label>
      </div>
      {res.error && <ErrorBox error={res.error} onRetry={res.reload} />}
      {empty ? (
        <Empty title="찾을 낱말을 입력하세요.">원문과 요약에서 찾습니다. 두 글자 낱말(예: 지각)도 찾을 수 있습니다.</Empty>
      ) : (
        r && (
          <>
            {r.students.length > 0 && (
              <section className="panel">
                <h2>학생</h2>
                <div className="row gap wrap">
                  {r.students.map((s) => (
                    <button key={s.id} onClick={() => navigate("students", { studentId: s.id })}>
                      {s.class_label ?? ""} {s.name} · 관찰 {s.observation_count}
                    </button>
                  ))}
                </div>
              </section>
            )}
            <section className="panel">
              <h2>
                기록 <span className="count">{r.notes.length}</span>
              </h2>
              {r.notes.length === 0 && <p className="muted">일치하는 기록이 없습니다.</p>}
              <ul className="result-list">
                {r.notes.map((h) => (
                  <li key={h.note.id}>
                    <button className="result" onClick={() => navigate("notes", { noteId: h.note.id })}>
                      <div className="result-head">
                        <StatusBadge status={h.note.status} />
                        <time>{fmtDateTime(h.note.created_at)}</time>
                      </div>
                      {h.note.summary && (
                        <p className="note-summary">
                          <Mark text={h.note.summary} terms={r.terms} />
                        </p>
                      )}
                      <p className="snippet">
                        <Mark text={h.snippet} terms={r.terms} />
                      </p>
                    </button>
                    <NoteChips note={h.note} />
                  </li>
                ))}
              </ul>
            </section>
            {(r.tasks.length > 0 || r.events.length > 0) && (
              <section className="panel">
                <h2>할 일·일정</h2>
                <ul className="plain-list">
                  {r.tasks.map((t) => (
                    <li key={`t${t.id}`}>
                      <button className="link" onClick={() => openEditor({ kind: "task", id: t.id })}>
                        {t.done ? "✓ " : ""}할 일 · {t.title}
                      </button>{" "}
                      <span className="muted">{t.due_date ? fmtDate(t.due_date) : "날짜 없음"}</span>
                    </li>
                  ))}
                  {r.events.map((e) => (
                    <li key={`e${e.id}`}>
                      <button className="link" onClick={() => openEditor({ kind: "event", id: e.id })}>
                        {LAYER_LABEL[e.layer]} · {e.title}
                      </button>{" "}
                      <span className="muted">{fmtDate(e.start_date)}</span>
                    </li>
                  ))}
                </ul>
              </section>
            )}
          </>
        )
      )}
    </div>
  );
}
