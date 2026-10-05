import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { Empty, ErrorBox } from "../components/Fields";
import { IconEdit, IconPlus, IconRecords, IconTrash } from "../components/Icons";
import { confirmDialog, Modal } from "../components/Modal";
import { DropOverlay, NEIS_ROSTER_TIP, NeisImportDialog, useExcelDrop } from "../components/NeisRosterImport";
import { RosterTable } from "../components/RosterTable";
import { fmtDate, fmtDateTime } from "../lib/format";
import { AREA_ORDER, AREA_SHORT, areaLabel } from "../lib/labels";
import { useLoad, useStore } from "../lib/store";
import type { Area, Observation, Student, StudentInput } from "../types";

function StudentForm({ initial, onClose, onSaved }: { initial: Student | null; onClose: () => void; onSaved: () => void }) {
  const { toast } = useStore();
  const [name, setName] = useState(initial?.name ?? "");
  const [label, setLabel] = useState(initial?.class_label ?? "");
  const [num, setNum] = useState(initial?.number ? String(initial.number) : "");
  const [memo, setMemo] = useState(initial?.memo ?? "");
  const save = async () => {
    const input: StudentInput = { name, class_label: label || null, number: num ? Number(num) : null, memo };
    try {
      if (initial) await api.updateStudent(initial.id, input);
      else await api.createStudent(input);
      toast("저장했습니다.");
      onSaved();
      onClose();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <Modal
      title={initial ? "학생 정보 수정" : "학생 추가"}
      onClose={onClose}
      footer={
        <>
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={() => void save()} disabled={!name.trim()}>
            저장
          </button>
        </>
      }
    >
      <div className="row gap">
        <label className="field">
          <span>학급 (예: 2-1)</span>
          <input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="2-1" />
        </label>
        <label className="field">
          <span>번호</span>
          <input value={num} onChange={(e) => setNum(e.target.value.replace(/\D/g, ""))} inputMode="numeric" />
        </label>
        <label className="field grow">
          <span>이름</span>
          <input value={name} onChange={(e) => setName(e.target.value)} />
        </label>
      </div>
      <label className="field">
        <span>메모 (선택)</span>
        <textarea rows={2} value={memo} onChange={(e) => setMemo(e.target.value)} />
      </label>
      {initial?.needs_review && <p className="hint">기록에서 자동으로 등록된 학생입니다. 학급·번호를 확인하고 저장하면 ‘확인 필요’ 표시가 사라집니다.</p>}
    </Modal>
  );
}

function ImportDialog({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const { toast } = useStore();
  const [text, setText] = useState("");
  const [result, setResult] = useState<string | null>(null);
  const run = async () => {
    try {
      const r = await api.importRoster(text);
      setResult(`${r.added}명 추가, ${r.skipped}명은 이미 있어 건너뜀${r.problems.length ? `\n이해하지 못한 줄:\n${r.problems.join("\n")}` : ""}`);
      onDone();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <Modal
      title="명단 붙여넣기"
      onClose={onClose}
      footer={
        <>
          <button onClick={onClose}>닫기</button>
          <button className="primary" onClick={() => void run()} disabled={!text.trim()}>
            가져오기
          </button>
        </>
      }
    >
      <p className="hint">한 줄에 한 명씩 적거나, 엑셀·NEIS 명단에서 학년·반·번호·이름 칸을 복사해 붙여 넣으세요.</p>
      <pre className="example">{"2-1 3 홍길동\n2학년 1반 7번 성춘향\n2\t1\t12\t이몽룡"}</pre>
      <textarea rows={10} value={text} onChange={(e) => setText(e.target.value)} placeholder="여기에 붙여 넣기" />
      {result && <pre className="result">{result}</pre>}
    </Modal>
  );
}

function ObservationRow({ o, students }: { o: Observation; students: Student[] }) {
  const { toast, bump, navigate } = useStore();
  const [open, setOpen] = useState(false);
  const [edit, setEdit] = useState(false);
  const [area, setArea] = useState<Area>(o.area);
  const [content, setContent] = useState(o.content);
  const [day, setDay] = useState(o.observed_on);
  const [sid, setSid] = useState<number | null>(o.student_id);
  const save = async () => {
    try {
      await api.updateObservation(o.id, { student_id: sid, area, content, observed_on: day });
      setEdit(false);
      toast("관찰 기록을 고쳤습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const del = async () => {
    if (!(await confirmDialog("이 관찰 기록을 지울까요? (원문 기록은 남습니다)", { okText: "지우기", danger: true }))) return;
    await api.deleteObservation(o.id).catch((e) => toast(errorText(e), "error"));
    bump();
  };
  return (
    <li className="obs-row">
      <div className="obs-date">{fmtDate(o.observed_on)}</div>
      <div className="obs-main">
        {!edit ? (
          <>
            <div className="obs-top">
              <span className={`chip area-${o.area}`}>{areaLabel(o.area)}</span>
              <span className={`tag src-${o.source}`}>{o.source === "manual" ? "직접" : o.edited ? "AI·수정됨" : "AI"}</span>
            </div>
            <p className="obs-content">{o.content}</p>
            {o.note_raw && (
              <button className="link" onClick={() => setOpen((x) => !x)}>
                {open ? "원문 접기" : "원문 보기"}
              </button>
            )}
            {open && o.note_raw && (
              <div className="raw-box small">
                <div className="raw-head">
                  원문 · {fmtDateTime(o.note_created_at)}{" "}
                  <button className="link" onClick={() => o.note_id && navigate("notes", { noteId: o.note_id })}>
                    기록으로 가기
                  </button>
                </div>
                <p className="raw-text">{o.note_raw}</p>
              </div>
            )}
          </>
        ) : (
          <div className="obs-edit">
            <div className="row gap wrap">
              <select value={area} onChange={(e) => setArea(e.target.value as Area)}>
                {[...AREA_ORDER, "none" as Area].map((a) => (
                  <option key={a} value={a}>
                    {areaLabel(a)}
                  </option>
                ))}
              </select>
              <input type="date" value={day} onChange={(e) => setDay(e.target.value)} />
              <select value={sid ?? ""} onChange={(e) => setSid(e.target.value ? Number(e.target.value) : null)}>
                <option value="">학생 미지정</option>
                {students.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.class_label ?? ""} {s.name}
                  </option>
                ))}
              </select>
            </div>
            <textarea rows={2} value={content} onChange={(e) => setContent(e.target.value)} />
            <div className="row gap">
              <button onClick={() => setEdit(false)}>취소</button>
              <button className="primary" onClick={() => void save()}>
                저장
              </button>
            </div>
          </div>
        )}
      </div>
      {!edit && (
        <div className="obs-actions">
          <button className="icon-btn" onClick={() => setEdit(true)} aria-label="수정" title="수정">
            <IconEdit size={16} />
          </button>
          <button className="icon-btn" onClick={() => void del()} aria-label="삭제" title="삭제">
            <IconTrash size={16} />
          </button>
        </div>
      )}
    </li>
  );
}

function birthText(b: string | null): string {
  if (!b) return "";
  if (b.length === 10) return `${b.slice(0, 4)}년 ${Number(b.slice(5, 7))}월 ${Number(b.slice(8))}일`;
  return `${Number(b.slice(0, 2))}월 ${Number(b.slice(3))}일`;
}

/** 학생 정보 (나이스 명렬표 항목). 주소·우편번호·주민등록번호는 눌러야 보인다. */
function StudentInfo({ s, onChanged }: { s: Student; onChanged: () => void }) {
  const { toast } = useStore();
  const [show, setShow] = useState(false);
  const plain: Array<[string, string]> = (
    [
      ["학생개인번호", s.student_no],
      ["성별", s.gender],
      ["생년월일", birthText(s.birthday)],
      ["이전학적", s.prev_record],
      ["비고", s.remark],
    ] as Array<[string, string]>
  ).filter(([, v]) => v);
  const address = [s.postal_code && `(${s.postal_code})`, s.address].filter(Boolean).join(" ");
  const hidden = !!(address || s.rrn);
  if (!plain.length && !hidden) return null;
  const clearRrn = async () => {
    if (!(await confirmDialog("이 학생의 주민등록번호를 지울까요?", { okText: "지우기", danger: true }))) return;
    try {
      await api.setStudentField(s.id, "rrn", "");
      onChanged();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <section className="panel student-info">
      <h2>학생 정보</h2>
      <dl className="spec">
        {plain.map(([k, v]) => (
          <div key={k} className="spec-row">
            <dt>{k}</dt>
            <dd>{v}</dd>
          </div>
        ))}
        {hidden && !show && (
          <div className="spec-row">
            <dt>주소 등</dt>
            <dd>
              <button className="link" onClick={() => setShow(true)}>
                눌러서 보기
              </button>
            </dd>
          </div>
        )}
        {show && address && (
          <div className="spec-row">
            <dt>주소</dt>
            <dd>{address}</dd>
          </div>
        )}
        {show && s.rrn && (
          <div className="spec-row">
            <dt>주민등록번호</dt>
            <dd>
              {s.rrn}{" "}
              <button className="link danger" onClick={() => void clearRrn()}>
                지우기
              </button>
            </dd>
          </div>
        )}
      </dl>
      {show && (
        <button className="link" onClick={() => setShow(false)}>
          다시 가리기
        </button>
      )}
    </section>
  );
}

function StudentDetailView({ id, onBack, students }: { id: number; onBack: () => void; students: Student[] }) {
  const { navigate, toast, bump } = useStore();
  const d = useLoad(() => api.studentDetail(id), [id]);
  const [edit, setEdit] = useState(false);
  const [areaFilter, setAreaFilter] = useState<string>("all");
  const [mergeInto, setMergeInto] = useState<string>("");
  if (d.error) return <ErrorBox error={d.error} onRetry={d.reload} />;
  if (!d.data) return <p className="muted">불러오는 중…</p>;
  const s = d.data.student;
  const obs = d.data.observations.filter((o) => areaFilter === "all" || o.area === areaFilter);
  const others = students.filter((x) => x.id !== s.id && x.name === s.name);
  const del = async () => {
    if (!(await confirmDialog(`${s.name} 학생과 이 학생의 관찰 기록·초안을 지울까요? 원문 기록은 남습니다.`, { okText: "지우기", danger: true }))) return;
    await api.deleteStudent(s.id).catch((e) => toast(errorText(e), "error"));
    bump();
    onBack();
  };
  const merge = async () => {
    const into = Number(mergeInto);
    if (!into) return;
    if (!(await confirmDialog("이 학생의 기록을 고른 학생으로 옮기고, 이 학생은 지웁니다. 계속할까요?", { okText: "합치기" }))) return;
    await api.mergeStudents(s.id, into).catch((e) => toast(errorText(e), "error"));
    bump();
    onBack();
  };
  return (
    <div className="student-detail">
      <div className="page-head">
        <div>
          <button className="link" onClick={onBack}>
            ← 학생 목록
          </button>
          <h1>
            {s.class_label && <span className="muted">{s.class_label} {s.number ? `${s.number}번` : ""}</span>} {s.name}
            {s.needs_review && <span className="tag warn">확인 필요</span>}
          </h1>
        </div>
        <div className="head-actions">
          <button onClick={() => navigate("records", { studentId: s.id })}>
            <IconRecords size={15} /> 학생부 자료
          </button>
          <button onClick={() => setEdit(true)}>
            <IconEdit size={15} /> 정보 수정
          </button>
          <button className="ghost danger" onClick={() => void del()}>
            <IconTrash size={15} /> 삭제
          </button>
        </div>
      </div>
      {s.memo && <p className="page-desc">{s.memo}</p>}
      <StudentInfo s={s} onChanged={d.reload} />
      <div className="area-summary">
        <button className={`area-pill ${areaFilter === "all" ? "on" : ""}`} onClick={() => setAreaFilter("all")}>
          전체 <b>{d.data.observations.length}</b>
        </button>
        {[...AREA_ORDER, "none" as Area].map((a) => (
          <button key={a} className={`area-pill area-${a} ${areaFilter === a ? "on" : ""}`} onClick={() => setAreaFilter(a)} disabled={!s.area_counts[a]}>
            {AREA_SHORT[a]} <b>{s.area_counts[a] ?? 0}</b>
          </button>
        ))}
      </div>
      {s.needs_review && others.length > 0 && (
        <div className="callout soft">
          <span>같은 이름의 학생이 있습니다. 같은 학생이면 합쳐 주세요.</span>
          <select value={mergeInto} onChange={(e) => setMergeInto(e.target.value)}>
            <option value="">합칠 학생 고르기</option>
            {others.map((o) => (
              <option key={o.id} value={o.id}>
                {o.class_label ?? "반 미지정"} {o.number ? `${o.number}번` : ""} {o.name}
              </option>
            ))}
          </select>
          <button onClick={() => void merge()} disabled={!mergeInto}>
            합치기
          </button>
        </div>
      )}
      <section className="panel">
        <h2>
          관찰 기록 <span className="count">{obs.length}</span>
        </h2>
        {obs.length === 0 ? <Empty title="이 영역의 관찰 기록이 없습니다." /> : <ul className="obs-list">{obs.map((o) => <ObservationRow key={o.id} o={o} students={students} />)}</ul>}
      </section>
      <section className="panel">
        <h2>
          연결된 원문 기록 <span className="count">{d.data.notes.length}</span>
        </h2>
        <ul className="plain-list">
          {d.data.notes.map((n) => (
            <li key={n.id}>
              <button className="link" onClick={() => navigate("notes", { noteId: n.id })}>
                {fmtDateTime(n.created_at)}
              </button>{" "}
              {n.summary ?? n.raw_text.slice(0, 80)}
            </li>
          ))}
        </ul>
      </section>
      {edit && <StudentForm initial={s} onClose={() => setEdit(false)} onSaved={d.reload} />}
    </div>
  );
}

const VIEW_KEY = "tn.students.view";

export function StudentsPage() {
  const { params, bump } = useStore();
  const [view, setViewState] = useState<"cards" | "roster">(() => {
    try {
      return window.localStorage.getItem(VIEW_KEY) === "roster" ? "roster" : "cards";
    } catch {
      return "cards";
    }
  });
  const setView = (v: "cards" | "roster") => {
    setViewState(v);
    try {
      window.localStorage.setItem(VIEW_KEY, v);
    } catch {
      /* 저장 못 해도 동작 */
    }
  };
  const [query, setQuery] = useState("");
  const [classId, setClassId] = useState<number | null>(null);
  const [selected, setSelected] = useState<number | null>(params.studentId ?? null);
  const [adding, setAdding] = useState(false);
  const [importing, setImporting] = useState(false);
  const [neis, setNeis] = useState<{ paths: string[]; seq: number } | null>(null);
  const dragging = useExcelDrop((paths) => setNeis((cur) => ({ paths, seq: (cur?.seq ?? 0) + 1 })));
  const classes = useLoad(() => api.listClasses(), []);
  const list = useLoad(() => api.listStudents(query || null, classId), [query, classId]);
  const all = useLoad(() => api.listStudents(null, null), []);
  useEffect(() => {
    if (params.studentId) setSelected(params.studentId);
  }, [params.studentId]);

  const neisParts = (
    <>
      {dragging && <DropOverlay />}
      {neis && <NeisImportDialog incoming={neis} onClose={() => setNeis(null)} />}
    </>
  );
  if (selected !== null)
    return (
      <div className="page">
        <StudentDetailView id={selected} onBack={() => setSelected(null)} students={all.data ?? []} />
        {neisParts}
      </div>
    );
  const head = (
    <div className="page-head no-print">
      <h1>학생</h1>
      <div className="head-actions">
        <div className="seg">
          <button className={view === "cards" ? "on" : ""} onClick={() => setView("cards")}>
            카드
          </button>
          <button className={view === "roster" ? "on" : ""} onClick={() => setView("roster")}>
            학급명렬표
          </button>
        </div>
        {view === "cards" && <input className="search-input" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="이름·학급 찾기" />}
        <button onClick={() => setImporting(true)}>명단 붙여넣기</button>
        <button onClick={() => setNeis({ paths: [], seq: 0 })} title={NEIS_ROSTER_TIP}>
          나이스 명렬표 가져오기
        </button>
        <button className="primary" onClick={() => setAdding(true)}>
          <IconPlus size={15} /> 학생 추가
        </button>
      </div>
    </div>
  );
  if (view === "roster") {
    return (
      <div className="page">
        {head}
        <RosterTable onOpen={(id) => setSelected(id)} />
        {adding && <StudentForm initial={null} onClose={() => setAdding(false)} onSaved={bump} />}
        {importing && <ImportDialog onClose={() => setImporting(false)} onDone={bump} />}
        {neisParts}
      </div>
    );
  }
  return (
    <div className="page">
      {head}
      <div className="class-filter">
        <button className={classId === null ? "on" : ""} onClick={() => setClassId(null)}>
          전체
        </button>
        {classes.data?.map((c) => (
          <button key={c.id} className={classId === c.id ? "on" : ""} onClick={() => setClassId(c.id)}>
            {c.label} <small>{c.student_count}</small>
          </button>
        ))}
      </div>
      {list.error && <ErrorBox error={list.error} onRetry={list.reload} />}
      {list.data && list.data.length === 0 && (
        <Empty title="학생이 없습니다.">
          나이스에서 받은 명렬표 엑셀 파일을 가져오거나(이 화면에 끌어다 놓아도 됩니다), 명단을 붙여 넣으세요. 기록에 “2-1 홍길동”처럼 학급과 이름을 적어도 자동으로 등록됩니다.
        </Empty>
      )}
      <div className="student-grid">
        {list.data?.map((s) => (
          <button key={s.id} className={`student-card ${s.needs_review ? "review" : ""}`} onClick={() => setSelected(s.id)}>
            <div className="sc-top">
              <span className="sc-class">{s.class_label ?? "반 미지정"}{s.number ? ` · ${s.number}번` : ""}</span>
              {s.needs_review && <span className="tag warn">확인 필요</span>}
            </div>
            <div className="sc-name">{s.name}</div>
            <div className="sc-count">
              관찰 <b>{s.observation_count}</b> · 기록 <b>{s.note_count}</b>
            </div>
            <div className="sc-areas">
              {AREA_ORDER.filter((a) => s.area_counts[a]).map((a) => (
                <span key={a} className={`mini-pill area-${a}`}>
                  {AREA_SHORT[a]} {s.area_counts[a]}
                </span>
              ))}
            </div>
            {s.last_noted_at && <div className="sc-last muted">최근 {fmtDate(s.last_noted_at.slice(0, 10))}</div>}
          </button>
        ))}
      </div>
      {adding && <StudentForm initial={null} onClose={() => setAdding(false)} onSaved={bump} />}
      {importing && <ImportDialog onClose={() => setImporting(false)} onDone={bump} />}
      {neisParts}
    </div>
  );
}
