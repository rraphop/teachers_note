import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
import { api, errorText } from "../api";
import { todayText } from "../components/ExportTools";
import { Empty, ErrorBox } from "../components/Fields";
import { IconCopy, IconSpark } from "../components/Icons";
import { confirmDialog } from "../components/Modal";
import { fmtDate, fmtDateTime } from "../lib/format";
import { AREA_ORDER, AREA_SHORT, areaLabel, RECORD_TABS } from "../lib/labels";
import { useLoad, useStore } from "../lib/store";
import type { Area, DraftCheck, Observation, RecordStatus, RecordTab, Student } from "../types";

const STATUS_TEXT: Record<RecordStatus, string> = { none: "자료 없음", refs: "참고자료만", draft: "초안", final: "최종안" };
const STATUS_MARK: Record<RecordStatus, string> = { none: "·", refs: "○", draft: "◐", final: "●" };

function Highlighted({ text, words }: { text: string; words: string[] }) {
  const esc = words.filter((w) => w.length > 0).map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (esc.length === 0) return <>{text}</>;
  const re = new RegExp(`(${esc.join("|")})`, "g");
  return (
    <>
      {text.split(re).map((part, i) =>
        words.includes(part) ? (
          <mark key={i} title="참고자료에 없는 표현입니다. 사실인지 확인해 주세요.">
            {part}
          </mark>
        ) : (
          <span key={i}>{part}</span>
        ),
      )}
    </>
  );
}

/** 참고자료 한 줄: 날짜·내용·원문, 영역 옮기기, 과목(활동) 고르기, 빼기 */
function RefRow({ o, subjects, onChanged }: { o: Observation; subjects: string[]; onChanged: () => void }) {
  const { toast, navigate } = useStore();
  const update = async (patch: { area?: Area; subject?: string | null; excluded?: boolean }) => {
    try {
      await api.updateObservation(o.id, {
        student_id: o.student_id,
        area: patch.area ?? o.area,
        content: o.content,
        observed_on: o.observed_on,
        subject: patch.area && patch.area !== o.area ? null : (patch.subject ?? o.subject),
        excluded: patch.excluded ?? o.excluded,
      });
      onChanged();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const hasSubject = o.area === "subject" || o.area === "free_semester";
  return (
    <li className={`ref-row ${o.excluded ? "excluded" : ""}`}>
      <div className="ref-main">
        <span className="ref-date">{fmtDate(o.observed_on)}</span>
        <span className="ref-text">{o.content}</span>
        {o.excluded && <span className="tag">뺌</span>}
      </div>
      <div className="ref-tools">
        {o.note_raw && (
          <details className="inline-raw">
            <summary>원문</summary>
            <p className="raw-text">{o.note_raw}</p>
            {o.note_id && (
              <button className="link" onClick={() => navigate("notes", { noteId: o.note_id ?? undefined })}>
                기록으로 가기 ({fmtDateTime(o.note_created_at)})
              </button>
            )}
          </details>
        )}
        <select value={o.area} onChange={(e) => void update({ area: e.target.value as Area })} title="다른 영역으로 옮기기" aria-label="영역 옮기기">
          {[...AREA_ORDER, "none" as Area].map((a) => (
            <option key={a} value={a}>
              {areaLabel(a)}
            </option>
          ))}
        </select>
        {hasSubject && (
          <select value={o.subject} onChange={(e) => void update({ subject: e.target.value })} title={o.area === "subject" ? "과목" : "활동"} aria-label="과목·활동">
            {Array.from(new Set([...subjects, o.subject])).map((s) => (
              <option key={s} value={s}>
                {s || (o.area === "subject" ? "과목 미지정" : "활동 미지정")}
              </option>
            ))}
          </select>
        )}
        <button className="ghost" onClick={() => void update({ excluded: !o.excluded })}>
          {o.excluded ? "다시 넣기" : "빼기"}
        </button>
      </div>
    </li>
  );
}

/** 칸 하나: 참고자료 + 최종안 입력 칸 */
function TabEditor({ studentId, tab, subjects, hasModel, onChanged }: { studentId: number; tab: RecordTab; subjects: string[]; hasModel: boolean; onChanged: () => void }) {
  const { toast } = useStore();
  const [body, setBody] = useState(tab.text?.body ?? "");
  const [check, setCheck] = useState<DraftCheck | null>(tab.text?.check ?? null);
  const [busy, setBusy] = useState<string | null>(null);
  const saved = tab.text?.body ?? "";
  const dirty = body.trim() !== saved.trim();
  const included = tab.refs.filter((o) => !o.excluded).length;
  useEffect(() => {
    const t = window.setTimeout(() => {
      api.checkRecordText(studentId, tab.area, tab.subject, body).then(setCheck).catch(() => undefined);
    }, 250);
    return () => window.clearTimeout(t);
  }, [body, studentId, tab.area, tab.subject]);

  const run = async (what: string, f: () => Promise<RecordTab>, ok: string) => {
    setBusy(what);
    try {
      const t = await f();
      setBody(t.text?.body ?? "");
      toast(ok);
      onChanged();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(null);
    }
  };
  const replaceOk = async () => !body.trim() || (await confirmDialog("지금 칸의 글을 새 글로 바꿀까요? (지금 글은 기록에 남지 않습니다)", { okText: "바꾸기" }));
  const gather = async () => {
    if (!(await replaceOk())) return;
    await run("gather", () => api.recordGather(studentId, tab.area, tab.subject), "참고자료를 날짜순으로 모아 붙였습니다. 문장으로 다듬어 주세요.");
  };
  const ai = async () => {
    if (!(await replaceOk())) return;
    await run("ai", () => api.recordAiDraft(studentId, tab.area, tab.subject), "AI 초안을 만들었습니다. 노란색 표현을 꼭 확인해 주세요.");
  };
  const saveText = async (final: boolean) =>
    run(final ? "final" : "save", () => api.saveRecordText(studentId, tab.area, tab.subject, body, final), final ? "최종안으로 저장했습니다." : body.trim() ? "초안으로 저장했습니다." : "칸의 글을 지웠습니다.");
  const copy = async () => {
    try {
      await writeText(body);
      toast("복사했습니다. 나이스에 붙여 넣기 전에 한 번 더 읽어 주세요.");
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const over = !!check?.over;
  return (
    <div className="tab-editor">
      <section className="panel">
        <h2>
          참고자료 <span className="count">{included}</span>
          {tab.refs.length > included && <small className="muted"> (뺀 것 {tab.refs.length - included})</small>}
        </h2>
        {tab.refs.length === 0 ? (
          <p className="muted small">이 칸에 모인 참고자료가 없습니다. 학생 이름을 넣어 기록하면 AI가 알맞은 영역에 모읍니다. 다른 칸의 참고자료를 이 영역으로 옮길 수도 있습니다.</p>
        ) : (
          <ul className="ref-list">
            {tab.refs.map((o) => (
              <RefRow key={o.id} o={o} subjects={subjects} onChanged={onChanged} />
            ))}
          </ul>
        )}
      </section>
      <section className={`panel final-box status-${tab.status}`}>
        <div className="final-head">
          <h2>최종안</h2>
          <span className={`rec-status st-${tab.status}`}>
            {STATUS_MARK[tab.status]} {STATUS_TEXT[tab.status]}
          </span>
          {tab.text && <small className="muted">{fmtDateTime(tab.text.updated_at)}</small>}
          <span className="grow" />
          <span className={`counter ${over ? "over" : ""}`}>
            {check?.chars ?? body.length}자{check?.limit_chars ? ` / ${check.limit_chars}자` : ""} · {check?.neis_bytes ?? 0}
            {check?.limit_bytes ? ` / ${check.limit_bytes}` : ""}바이트
          </span>
        </div>
        {over && <p className="err small">나이스 입력 한도({check?.limit_chars}자, {check?.limit_bytes}바이트)를 넘었습니다. 줄여 주세요. (한글 1자 3바이트, 영문·숫자·엔터 1바이트)</p>}
        <textarea rows={Math.max(6, Math.min(16, body.split("\n").length + 3))} value={body} onChange={(e) => setBody(e.target.value)} placeholder="참고자료를 바탕으로 교사가 쓰는 최종안입니다. [AI 초안 만들기]로 시작할 수 있습니다." />
        {check && check.novel_words.length > 0 && (
          <div className="novel">
            <strong>참고자료에 없는 표현 {check.novel_words.length}개</strong> — 사실인지 확인하고 필요 없으면 지워 주세요.
            <p className="preview">
              <Highlighted text={body} words={check.novel_words} />
            </p>
          </div>
        )}
        <div className="row gap wrap">
          <button onClick={() => void gather()} disabled={!!busy || included === 0} title="참고자료를 날짜순으로 모읍니다 (문장을 새로 만들지 않음)">
            참고자료 모아 붙이기
          </button>
          <button onClick={() => void ai()} disabled={!!busy || included === 0 || !hasModel} title={hasModel ? "이 칸의 참고자료만 근거로 초안을 만듭니다" : "AI 모델을 설치하면 쓸 수 있습니다"}>
            <IconSpark size={15} /> {busy === "ai" ? "만드는 중…" : "AI 초안 만들기"}
          </button>
          <span className="grow" />
          <button onClick={() => void copy()} disabled={!body.trim()}>
            <IconCopy size={15} /> 복사
          </button>
          <button onClick={() => void saveText(false)} disabled={!!busy || (!dirty && tab.status !== "final")}>
            {tab.status === "final" && !dirty ? "초안으로 되돌리기" : "초안으로 저장"}
          </button>
          <button className="primary" onClick={() => void saveText(true)} disabled={!!busy || !body.trim() || (!dirty && tab.status === "final")}>
            최종안으로 저장
          </button>
        </div>
      </section>
    </div>
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

function tabName(area: Area): string {
  return area === "autonomous" ? "창체(자율·자치)" : AREA_SHORT[area];
}

export function RecordsPage() {
  const { params, toast, bump } = useStore();
  const [q, setQ] = useState("");
  const [sid, setSid] = useState<number | null>(params.studentId ?? null);
  const [area, setArea] = useState<Area>("autonomous");
  const [subject, setSubject] = useState<string>("");
  const students = useLoad(() => api.listStudents(null, null), []);
  const classes = useLoad(() => api.listClasses(), []);
  const models = useLoad(() => api.modelCatalog(), []);
  const hasModel = !!models.data?.entries.some((e) => e.installed && e.selected);
  const record = useLoad(async () => (sid === null ? null : api.studentRecord(sid)), [sid]);
  useEffect(() => {
    if (params.studentId) setSid(params.studentId);
  }, [params.studentId]);

  const tabs = record.data ?? [];
  const subTabs = tabs.filter((t) => t.area === area);
  const sub = subTabs.some((t) => t.subject === subject) ? subject : (subTabs[0]?.subject ?? "");
  const tab = subTabs.find((t) => t.subject === sub) ?? null;
  const extraAreas = Array.from(new Set(tabs.filter((t) => !RECORD_TABS.includes(t.area)).map((t) => t.area)));
  const status = useLoad(() => api.recordStatus(area, sub), [area, sub]);
  const statusOf = (id: number): RecordStatus => status.data?.find((r) => r.student_id === id)?.status ?? "none";

  const groups = useMemo(() => {
    const list = (students.data ?? []).filter((s) => !q || s.name.includes(q) || (s.class_label ?? "").includes(q));
    const m = new Map<string, Student[]>();
    for (const s of list) {
      const k = s.class_label ?? "반 미지정";
      m.set(k, [...(m.get(k) ?? []), s]);
    }
    return Array.from(m.entries());
  }, [students.data, q]);
  const current = students.data?.find((s) => s.id === sid) ?? null;
  const subjectOptions = subTabs.map((t) => t.subject).filter(Boolean);

  const exportFinals = async () => {
    const cls = classes.data?.find((c) => c.id === current?.class_id) ?? null;
    const path = await save({ defaultPath: `학생부 최종안 ${cls?.label ?? "전체"} ${todayText()}.xlsx`, filters: [{ name: "엑셀", extensions: ["xlsx"] }] });
    if (!path) return;
    try {
      const r = await api.exportRecordFinals(cls?.id ?? null, path);
      toast(r.rows ? `최종안 ${r.rows}칸(학생 ${r.students}명)을 엑셀로 저장했습니다.` : "최종안으로 저장한 칸이 없어 빈 파일을 만들었습니다.");
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const refresh = () => {
    record.reload();
    status.reload();
    bump();
  };

  return (
    <div className="page records">
      <div className="page-head">
        <h1>학생부 자료</h1>
        <div className="head-actions">
          <button onClick={() => void exportFinals()} title="고른 학생의 학급(학생을 고르지 않았으면 전체) 최종안을 영역·과목 순으로 엑셀에 담습니다">
            {current?.class_label ? `${current.class_label} ` : ""}최종안 엑셀로 저장
          </button>
        </div>
      </div>
      <div className="notice">
        초안은 <b>교사가 검토하는 자료</b>입니다. 이 프로그램은 나이스에 자동으로 입력하지 않습니다. AI가 쓴 글을 그대로 나이스에 옮기는 것은 기재요령상 허용되지 않으니, 교사가 사실 여부와 기재요령을 확인해 고쳐 써
        주세요. 노란색 표시는 참고자료에 없는 표현입니다. 글자 수 한도는 2026학년도 중학교 기재요령(한글 1자 3바이트, 엔터 1바이트)을 따릅니다.
      </div>
      <UnassignedPanel students={students.data ?? []} />
      <div className="seg area-tabs" role="tablist" aria-label="학생부 영역">
        {[...RECORD_TABS, ...extraAreas].map((a) => (
          <button key={a} role="tab" aria-selected={area === a} className={area === a ? "on" : ""} onClick={() => setArea(a)}>
            {tabName(a)}
          </button>
        ))}
      </div>
      {(area === "subject" || area === "free_semester") && subTabs.length > 0 && (
        <div className="sub-tabs">
          {subTabs.map((t) => (
            <button key={t.subject} className={`chip-toggle ${t.subject === sub ? "on" : ""}`} onClick={() => setSubject(t.subject)}>
              {t.subject || (area === "subject" ? "과목 미지정" : "활동 미지정")} <small>{STATUS_MARK[t.status]}</small>
            </button>
          ))}
          {area === "subject" && subjectOptions.length === 0 && <small className="muted">설정 → 학교에서 ‘내 과목’을 고르면 과목별로 나뉩니다.</small>}
        </div>
      )}
      <div className="records-layout">
        <aside className="student-pick">
          <input className="search-input" value={q} onChange={(e) => setQ(e.target.value)} placeholder="학생 찾기" />
          <p className="small muted status-legend">● 최종안 ◐ 초안 ○ 참고자료만 · 없음</p>
          {students.data && students.data.length === 0 && <p className="muted small">학생이 없습니다. 학생 화면에서 나이스 명렬표를 가져오세요.</p>}
          {groups.map(([label, list]) => {
            const done = list.filter((s) => statusOf(s.id) === "final").length;
            return (
              <div key={label} className="pick-group">
                <div className="pick-class">
                  {label} <small className="muted">최종안 {done}/{list.length}</small>
                </div>
                <ul>
                  {list.map((s) => {
                    const st = statusOf(s.id);
                    return (
                      <li key={s.id}>
                        <button className={s.id === sid ? "on" : ""} onClick={() => setSid(s.id)} title={STATUS_TEXT[st]}>
                          <span className={`rec-mark st-${st}`}>{STATUS_MARK[st]}</span>
                          <span className="muted">{s.number ?? ""}</span> {s.name}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </div>
            );
          })}
        </aside>
        <div className="records-main">
          {sid === null || !current ? (
            <Empty title="왼쪽에서 학생을 고르세요.">학생별 참고자료가 영역 7개(창체 자율·자치, 교과, 행특, 동아리, 진로, 자유학기, 일상생활)로 모입니다.</Empty>
          ) : record.error ? (
            <ErrorBox error={record.error} onRetry={record.reload} />
          ) : !tab ? (
            <p className="muted">불러오는 중…</p>
          ) : (
            <>
              <h2 className="student-title">
                {current.class_label} {current.number ? `${current.number}번` : ""} {current.name}
                <span className="muted small">
                  {" "}
                  · {areaLabel(tab.area)}
                  {tab.subject ? ` (${tab.subject})` : ""}
                  {tab.limit_chars ? ` · 최대 ${tab.limit_chars}자` : ""}
                </span>
              </h2>
              {area === "daily" && <p className="hint">일상생활 활동상황은 특수교육 기본 교육과정 학생의 영역입니다(기재요령). 해당 학생이면 참고자료를 이 영역으로 옮겨 쓰세요.</p>}
              <TabEditor key={`${sid}-${tab.area}-${tab.subject}-${tab.text?.updated_at ?? ""}`} studentId={sid} tab={tab} subjects={subjectOptions} hasModel={hasModel} onChanged={refresh} />
            </>
          )}
        </div>
      </div>
    </div>
  );
}
