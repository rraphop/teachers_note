import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useEffect, useMemo, useState } from "react";
import { api, errorText } from "../api";
import { Empty, ErrorBox } from "../components/Fields";
import { IconCopy, IconSpark, IconTrash } from "../components/Icons";
import { confirmDialog } from "../components/Modal";
import { fmtDate, fmtDateTime } from "../lib/format";
import { AREA_ORDER, areaLabel } from "../lib/labels";
import { useLoad, useStore } from "../lib/store";
import type { AreaBlock, Draft, DraftCheck, Observation, Student } from "../types";

function Highlighted({ text, words }: { text: string; words: string[] }) {
  if (words.length === 0) return <>{text}</>;
  const esc = words.filter((w) => w.length > 0).map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (esc.length === 0) return <>{text}</>;
  const re = new RegExp(`(${esc.join("|")})`, "g");
  return (
    <>
      {text.split(re).map((part, i) =>
        words.includes(part) ? (
          <mark key={i} title="관찰 기록에 없는 표현입니다. 사실인지 확인해 주세요.">
            {part}
          </mark>
        ) : (
          <span key={i}>{part}</span>
        ),
      )}
    </>
  );
}

function DraftEditor({ draft, observations }: { draft: Draft; observations: Observation[] }) {
  const { toast, bump } = useStore();
  const [body, setBody] = useState(draft.body);
  const [check, setCheck] = useState<DraftCheck>(draft.check);
  const [reviewed, setReviewed] = useState(draft.reviewed);
  const dirty = body !== draft.body || reviewed !== draft.reviewed;
  useEffect(() => {
    const t = window.setTimeout(() => {
      api.checkDraft(draft.id, body).then(setCheck).catch(() => undefined);
    }, 300);
    return () => window.clearTimeout(t);
  }, [body, draft.id]);
  const save = async (rev = reviewed) => {
    try {
      await api.updateDraft(draft.id, body, rev);
      toast("초안을 저장했습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const copy = async () => {
    try {
      await writeText(body);
      toast("복사했습니다. 학생부에 붙여 넣기 전에 한 번 더 읽어 주세요.");
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const del = async () => {
    if (!(await confirmDialog("이 초안을 지울까요? 관찰 기록은 남습니다.", { okText: "지우기", danger: true }))) return;
    await api.deleteDraft(draft.id).catch((e) => toast(errorText(e), "error"));
    bump();
  };
  const sources = observations.filter((o) => draft.source_ids.includes(o.id));
  return (
    <div className={`draft ${reviewed ? "reviewed" : ""}`}>
      <div className="draft-head">
        <span className={`tag ${draft.generated_by === "ai" ? "src-ai" : "src-manual"}`}>{draft.generated_by === "ai" ? "AI 초안" : draft.generated_by === "template" ? "기본 초안" : "직접 작성"}</span>
        <span className="muted small">{fmtDateTime(draft.updated_at)}</span>
        <span className="grow" />
        <span className="counter">
          {check.chars}자 · NEIS 기준 약 {check.neis_bytes}바이트
        </span>
      </div>
      <textarea rows={Math.max(4, Math.min(12, body.split("\n").length + 2))} value={body} onChange={(e) => setBody(e.target.value)} />
      {check.novel_words.length > 0 && (
        <div className="novel">
          <strong>근거 기록에 없는 표현 {check.novel_words.length}개</strong> — 사실인지 확인하고 필요 없으면 지워 주세요.
          <p className="preview">
            <Highlighted text={body} words={check.novel_words} />
          </p>
        </div>
      )}
      <details>
        <summary>근거 관찰 기록 {sources.length}개</summary>
        <ul className="plain-list">
          {sources.map((o) => (
            <li key={o.id}>
              {fmtDate(o.observed_on)} · {o.content}
            </li>
          ))}
        </ul>
      </details>
      <div className="row gap">
        <label className="check">
          <input
            type="checkbox"
            checked={reviewed}
            onChange={(e) => {
              setReviewed(e.target.checked);
              void save(e.target.checked);
            }}
          />
          교사 검토 완료
        </label>
        <span className="grow" />
        <button className="ghost danger" onClick={() => void del()} aria-label="초안 삭제">
          <IconTrash size={15} />
        </button>
        <button onClick={() => void copy()}>
          <IconCopy size={15} /> 복사
        </button>
        <button className="primary" onClick={() => void save()} disabled={!dirty}>
          저장
        </button>
      </div>
    </div>
  );
}

function AreaSection({ block, studentId, hasModel }: { block: AreaBlock; studentId: number; hasModel: boolean }) {
  const { toast, bump } = useStore();
  const [picked, setPicked] = useState<number[]>(() => block.observations.map((o) => o.id));
  const [busy, setBusy] = useState(false);
  useEffect(() => setPicked(block.observations.map((o) => o.id)), [block.observations]);
  const toggle = (id: number) => setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));
  const make = async (ai: boolean) => {
    setBusy(true);
    try {
      if (ai) await api.generateDraftAi(studentId, block.area, picked);
      else await api.createDraftTemplate(studentId, block.area, picked);
      toast(ai ? "AI 초안을 만들었습니다. 표시된 표현을 꼭 확인해 주세요." : "기본 초안을 만들었습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className={`panel area-block area-${block.area}`}>
      <h2>
        {block.label} <span className="count">{block.observations.length}</span>
      </h2>
      {block.observations.length === 0 ? (
        <p className="muted small">연결된 관찰 기록이 없습니다.</p>
      ) : (
        <>
          <ul className="pick-list">
            {block.observations.map((o) => (
              <li key={o.id}>
                <label className="check">
                  <input type="checkbox" checked={picked.includes(o.id)} onChange={() => toggle(o.id)} />
                  <span className="muted">{fmtDate(o.observed_on)}</span> {o.content}
                </label>
                {o.note_raw && (
                  <details className="inline-raw">
                    <summary>원문</summary>
                    <p className="raw-text">{o.note_raw}</p>
                  </details>
                )}
              </li>
            ))}
          </ul>
          {block.area !== "none" && (
            <div className="row gap">
              <button onClick={() => void make(false)} disabled={busy || picked.length === 0} title="고른 관찰 기록을 날짜순으로 모읍니다 (문장을 새로 만들지 않음)">
                기본 초안 만들기
              </button>
              <button onClick={() => void make(true)} disabled={busy || picked.length === 0 || !hasModel} title={hasModel ? "고른 관찰 기록만 써서 문장 초안을 만듭니다" : "AI 모델을 설치하면 쓸 수 있습니다"}>
                <IconSpark size={15} /> {busy ? "만드는 중…" : "AI 초안 만들기"}
              </button>
              <small className="muted">고른 기록 {picked.length}개</small>
            </div>
          )}
        </>
      )}
      {block.drafts.map((d) => (
        <DraftEditor key={`${d.id}-${d.updated_at}`} draft={d} observations={block.observations} />
      ))}
    </section>
  );
}

function UnassignedPanel({ students }: { students: Student[] }) {
  const { toast, bump } = useStore();
  const list = useLoad(() => api.unassignedObservations(), []);
  const [choice, setChoice] = useState<Record<number, string>>({});
  if (!list.data || list.data.length === 0) return null;
  const assign = async (o: Observation) => {
    const sid = Number(choice[o.id]);
    if (!sid) return;
    try {
      await api.updateObservation(o.id, { student_id: sid, area: o.area, content: o.content, observed_on: o.observed_on });
      toast("학생에게 연결했습니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <details className="panel unassigned">
      <summary>
        학생이 정해지지 않은 관찰 기록 <span className="count">{list.data.length}</span>
      </summary>
      <p className="hint">“학생들이 수업 중…”처럼 이름 없이 쓴 관찰입니다. 특정 학생의 기록이면 학생을 골라 연결하세요.</p>
      <ul className="plain-list">
        {list.data.map((o) => (
          <li key={o.id} className="row gap wrap">
            <span className={`chip area-${o.area}`}>{areaLabel(o.area)}</span>
            <span className="grow">
              {fmtDate(o.observed_on)} · {o.content}
            </span>
            <select value={choice[o.id] ?? ""} onChange={(e) => setChoice({ ...choice, [o.id]: e.target.value })}>
              <option value="">학생 고르기</option>
              {students.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.class_label ?? ""} {s.name}
                </option>
              ))}
            </select>
            <button onClick={() => void assign(o)} disabled={!choice[o.id]}>
              연결
            </button>
          </li>
        ))}
      </ul>
    </details>
  );
}

export function RecordsPage() {
  const { params } = useStore();
  const [q, setQ] = useState("");
  const [sid, setSid] = useState<number | null>(params.studentId ?? null);
  const students = useLoad(() => api.listStudents(null, null), []);
  const models = useLoad(() => api.modelCatalog(), []);
  const hasModel = !!models.data?.entries.some((e) => e.installed && e.selected);
  const blocks = useLoad(async () => (sid === null ? null : api.recordOverview(sid)), [sid]);
  useEffect(() => {
    if (params.studentId) setSid(params.studentId);
  }, [params.studentId]);
  const filtered = useMemo(() => (students.data ?? []).filter((s) => !q || s.name.includes(q) || (s.class_label ?? "").includes(q)), [students.data, q]);
  const current = students.data?.find((s) => s.id === sid) ?? null;

  return (
    <div className="page records">
      <div className="page-head">
        <h1>학생부 자료</h1>
      </div>
      <div className="notice">
        초안은 <b>교사가 검토하는 자료</b>입니다. 이 프로그램은 학생부(NEIS)에 자동으로 입력하지 않습니다. 노란색 표시는 근거 관찰 기록에 없는 표현입니다.
      </div>
      <UnassignedPanel students={students.data ?? []} />
      <div className="records-layout">
        <aside className="student-pick">
          <input className="search-input" value={q} onChange={(e) => setQ(e.target.value)} placeholder="학생 찾기" />
          {students.data && students.data.length === 0 && <p className="muted small">학생이 없습니다.</p>}
          <ul>
            {filtered.map((s) => (
              <li key={s.id}>
                <button className={s.id === sid ? "on" : ""} onClick={() => setSid(s.id)}>
                  <span className="muted">{s.class_label ?? ""}</span> {s.name}
                  <small>{s.observation_count}</small>
                </button>
              </li>
            ))}
          </ul>
        </aside>
        <div className="records-main">
          {sid === null || !current ? (
            <Empty title="왼쪽에서 학생을 고르세요.">학생별 누적 관찰 기록이 7개 영역으로 나뉘어 보입니다.</Empty>
          ) : blocks.error ? (
            <ErrorBox error={blocks.error} onRetry={blocks.reload} />
          ) : (
            <>
              <h2 className="student-title">
                {current.class_label} {current.number ? `${current.number}번` : ""} {current.name}
              </h2>
              <nav className="area-jump">
                {AREA_ORDER.map((a) => (
                  <a key={a} href={`#area-${a}`} className={`area-pill area-${a}`}>
                    {areaLabel(a)} <b>{blocks.data?.find((b) => b.area === a)?.observations.length ?? 0}</b>
                  </a>
                ))}
              </nav>
              {blocks.data?.map((b) => (
                <div id={`area-${b.area}`} key={b.area}>
                  <AreaSection block={b} studentId={sid} hasModel={hasModel} />
                </div>
              ))}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
