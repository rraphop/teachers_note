import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { bytes } from "../lib/format";
import { useLoad, useStore } from "../lib/store";
import type { ArchiveResult, NewYearPreview, YearInfo } from "../types";
import { pickFolder } from "./ExportTools";
import { Modal } from "./Modal";

const SCHOOL_KEY = "tn.archive.school";
const TEACHER_KEY = "tn.archive.teacher";

function readLocal(k: string): string {
  try {
    return window.localStorage.getItem(k) ?? "";
  } catch {
    return "";
  }
}
function writeLocal(k: string, v: string) {
  try {
    window.localStorage.setItem(k, v);
  } catch {
    /* 저장 못 해도 동작 */
  }
}

const dateOnly = (s: string | null) => (s ? s.slice(0, 10) : "");

/** 학년도 마감 기록물 만들기 */
function YearEndDialog({ year, onClose, onDone }: { year: YearInfo; onClose: () => void; onDone: () => void }) {
  const { toast } = useStore();
  const [parts, setParts] = useState({ business_pdf: true, student_pdf: true, excel: true, db_copy: true });
  const [school, setSchool] = useState(() => readLocal(SCHOOL_KEY));
  const [teacher, setTeacher] = useState(() => readLocal(TEACHER_KEY));
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ArchiveResult | null>(null);
  const y = year.school_year;
  const any = Object.values(parts).some(Boolean);

  const make = async () => {
    const path = await pickFolder(`${y}학년도 기록물을 만들 폴더 고르기`).catch(() => null);
    if (!path) return;
    writeLocal(SCHOOL_KEY, school.trim());
    writeLocal(TEACHER_KEY, teacher.trim());
    setBusy(true);
    try {
      setResult(await api.makeYearArchive({ school_year: y, path, ...parts, school_name: school, teacher_name: teacher }));
      onDone();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  if (result) {
    return (
      <Modal
        title={`${y}학년도 마감 기록물을 만들었습니다`}
        onClose={onClose}
        wide
        footer={
          <>
            <button onClick={() => void api.revealExport(result.folder).catch((e) => toast(errorText(e), "error"))}>탐색기에서 보기</button>
            <button className="primary" onClick={onClose}>
              닫기
            </button>
          </>
        }
      >
        <p className="export-path">
          <code>{result.folder}</code>
        </p>
        <table className="archive-files">
          <tbody>
            {result.files.map((f) => (
              <tr key={f.name}>
                <td>{f.name}</td>
                <td>{bytes(f.size)}</td>
                <td>
                  {f.pages !== null && `${f.pages}쪽 `}
                  {f.pdfa === true && <span className="tag ok">PDF/A 장기 보존용</span>}
                  {f.pdfa === false && (
                    <span className="tag warn" title={f.note ?? ""}>
                      일반 PDF
                    </span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="small muted">읽어보기.txt 와 SHA256SUMS.txt(파일이 바뀌지 않았는지 확인하는 값)도 함께 들어 있습니다.</p>
        <p className="hint warn">학생 정보가 들어 있는 파일(2·3·4번)은 학교 개인정보 보호 지침에 따라 보관하고, 보존 기간이 지나면 파기해 주세요.</p>
      </Modal>
    );
  }

  const row = (key: keyof typeof parts, title: string, desc: string, personal?: boolean) => (
    <label className="check archive-part">
      <input type="checkbox" checked={parts[key]} onChange={(e) => setParts({ ...parts, [key]: e.target.checked })} />
      <span>
        <b>{title}</b> {personal && <span className="tag warn">개인정보</span>}
        <br />
        <small className="muted">{desc}</small>
      </span>
    </label>
  );
  return (
    <Modal
      title={`${y}학년도 마감 기록물 만들기`}
      onClose={onClose}
      wide
      footer={
        <>
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={() => void make()} disabled={!any || busy}>
            {busy ? "만드는 중… (PDF 조판에 몇 초 걸립니다)" : "폴더 고르고 만들기…"}
          </button>
        </>
      }
    >
      <p className="small muted">
        {y}. 3. 1. ~ {y + 1}. 2. 말일의 자료로 ‘{y}학년도 교무수첩 기록물’ 폴더를 만듭니다. PDF는 오래 보관하기 위한 국제 표준 형식(PDF/A)으로 만듭니다.
      </p>
      <div className="archive-parts">
        {row("business_pdf", "1. 학사·업무 기록 (PDF)", "표지·차례, 월별 학사력 달력과 목록, 시간표·일과 시간, 담당업무와 업무 기록, 할 일")}
        {row("student_pdf", "2. 학생 기록 (PDF)", "학급 명렬표, 학생마다 한 쪽씩 영역별 관찰과 학생부 초안, 날짜별 기록 일지(원문)", true)}
        {row("excel", "3. 엑셀 원본", "위 내용의 표 자료 — 그 학년도 자료만, 항목마다 시트", true)}
        {row("db_copy", "4. 프로그램 백업 (.db)", "이 시점의 데이터 전체 — 나중에 프로그램에서 다시 열 수 있음", true)}
      </div>
      <div className="row gap wrap">
        <label className="field grow">
          <span>학교 이름 (표지에만, 선택)</span>
          <input value={school} onChange={(e) => setSchool(e.target.value)} placeholder="예: OO중학교" />
        </label>
        <label className="field grow">
          <span>교사 이름 (표지에만, 선택)</span>
          <input value={teacher} onChange={(e) => setTeacher(e.target.value)} placeholder="예: 홍길동" />
        </label>
      </div>
      <p className="hint warn">만든 파일에는 암호가 없습니다. 학생 정보가 들어가는 파일은 학교 규정에 맞는 곳(보안 USB 등)에 보관해 주세요.</p>
    </Modal>
  );
}

/** 새 학년도 시작 */
function NewYearDialog({ onClose, onArchiveFirst }: { onClose: () => void; onArchiveFirst: (year: number) => void }) {
  const { toast, bump } = useStore();
  const [target, setTarget] = useState<number | null>(null);
  const [p, setP] = useState<NewYearPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [opt, setOpt] = useState({ archive_students: true, copy_bell: true, copy_timetables: false, keep_duties: true });
  const [busy, setBusy] = useState(false);
  const [base, setBase] = useState<number | null>(null);

  useEffect(() => {
    setError(null);
    api
      .newYearPreview(target)
      .then((x) => {
        setP(x);
        if (base === null) setBase(x.new_year);
      })
      .catch((e) => setError(errorText(e)));
  }, [target, base]);

  const start = async () => {
    if (!p) return;
    setBusy(true);
    try {
      const r = await api.startNewYear({ new_year: p.new_year, ...opt });
      const parts = [
        r.archived_students > 0 ? `학생 ${r.archived_students}명 보관` : null,
        r.bell_copied ? "일과 시간 복사" : null,
        r.timetables_copied > 0 ? `시간표 ${r.timetables_copied}개 복사` : null,
        r.duties_paused > 0 ? `담당업무 ${r.duties_paused}개 쉼` : null,
      ].filter(Boolean);
      toast(`${r.new_year}학년도를 시작했습니다.${parts.length ? ` (${parts.join(", ")})` : ""} 시작 전 데이터는 ‘새 학년도 전’ 백업으로 남아 있습니다.`);
      bump();
      onClose();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  const years = base === null ? [] : [base - 1, base, base + 1];
  const canCopy = !!p?.bell_source;
  return (
    <Modal
      title="새 학년도 시작"
      onClose={onClose}
      wide
      footer={
        <>
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={() => void start()} disabled={!p || busy}>
            {busy ? "시작하는 중…" : p ? `${p.new_year}학년도 시작` : "시작"}
          </button>
        </>
      }
    >
      {error && <p className="error-box">{error}</p>}
      {!p ? (
        <p className="muted">준비하는 중…</p>
      ) : (
        <>
          <div className="row gap">
            <label className="field">
              <span>새 학년도</span>
              <select value={p.new_year} onChange={(e) => setTarget(Number(e.target.value))}>
                {years.map((y) => (
                  <option key={y} value={y}>
                    {y}학년도 ({y}. 3. ~ {y + 1}. 2.)
                  </option>
                ))}
              </select>
            </label>
          </div>
          {p.prev_archived_at ? (
            <p className="hint ok">{p.prev_year}학년도 마감 기록물을 {dateOnly(p.prev_archived_at)}에 만들었습니다.</p>
          ) : (
            <div className="year-warn">
              <span>{p.prev_year}학년도 마감 기록물을 아직 만들지 않았습니다. 학생을 보관 처리하기 전에 먼저 만들어 두기를 권합니다.</span>
              <button onClick={() => onArchiveFirst(p.prev_year)}>{p.prev_year}학년도 마감 기록물 먼저 만들기</button>
            </div>
          )}
          {p.started_year !== null && p.started_year >= p.new_year && <p className="hint warn">이미 {p.started_year}학년도를 시작했습니다. 다시 시작해도 괜찮지만, 이미 옮긴 것은 그대로 둡니다.</p>}
          <div className="archive-parts">
            <label className="check archive-part">
              <input type="checkbox" checked={opt.archive_students} onChange={(e) => setOpt({ ...opt, archive_students: e.target.checked })} />
              <span>
                <b>지난 학년도 학생 {p.students_to_archive}명 보관 처리</b>
                <br />
                <small className="muted">
                  학생 목록·명렬표·기록 분류에서 숨깁니다. 관찰 기록과 학생부 자료는 지우지 않고 그대로 둡니다.
                  {p.prev_classes.length > 0 && ` (학급: ${p.prev_classes.join(", ")})`}
                </small>
              </span>
            </label>
            <label className="check archive-part">
              <input type="checkbox" checked={opt.copy_bell && canCopy} disabled={!canCopy || p.new_has_bell} onChange={(e) => setOpt({ ...opt, copy_bell: e.target.checked })} />
              <span>
                <b>일과 시간 이어 쓰기</b>
                <br />
                <small className="muted">
                  {!canCopy
                    ? "지난 학년도에 저장한 일과 시간이 없습니다."
                    : p.new_has_bell
                      ? `${p.new_year}학년도 1학기 일과 시간이 이미 있어 그대로 둡니다.`
                      : `${p.bell_source} 일과 시간(등교·교시·점심·방과후)을 ${p.new_year}학년도 1학기로 복사합니다.`}
                </small>
              </span>
            </label>
            <label className="check archive-part">
              <input type="checkbox" checked={opt.copy_timetables && canCopy} disabled={!canCopy || p.timetables_source === 0} onChange={(e) => setOpt({ ...opt, copy_timetables: e.target.checked })} />
              <span>
                <b>시간표도 복사</b>
                <br />
                <small className="muted">
                  {p.timetables_source > 0 ? `${p.bell_source} 시간표 ${p.timetables_source}개(개인·학급·전체)를 새 학년도 1학기로 복사합니다. 이미 있는 시간표는 덮어쓰지 않습니다.` : "복사할 시간표가 없습니다."}
                </small>
              </span>
            </label>
            <label className="check archive-part">
              <input type="checkbox" checked={opt.keep_duties} onChange={(e) => setOpt({ ...opt, keep_duties: e.target.checked })} />
              <span>
                <b>담당업무 {p.active_duties}개 그대로 쓰기</b>
                <br />
                <small className="muted">끄면 모든 담당업무를 ‘쉼’으로 바꿉니다. 업무 기록은 지우지 않습니다.</small>
              </span>
            </label>
          </div>
          <p className="small muted">시작하기 전에 자동으로 백업합니다. 잘못 눌렀다면 설정 → 데이터의 백업 목록에서 ‘새 학년도 전’ 백업으로 되돌릴 수 있습니다.</p>
        </>
      )}
    </Modal>
  );
}

/** 설정 화면: 학년도 마감과 새 학년도 */
export function YearPanel() {
  const { version } = useStore();
  const years = useLoad(() => api.schoolYears(), [version]);
  const [archiving, setArchiving] = useState<YearInfo | null>(null);
  const [newYear, setNewYear] = useState(false);

  const openArchive = (y: number) => {
    const info = years.data?.find((x) => x.school_year === y) ?? {
      school_year: y,
      current: false,
      notes: 0,
      students: 0,
      observations: 0,
      events: 0,
      timetable_cells: 0,
      archived_at: null,
    };
    setNewYear(false);
    setArchiving(info);
  };

  return (
    <section className="panel" id="set-year">
      <h2>학년도 마감과 새 학년도</h2>
      <p className="small muted">학년도가 끝나면 한 해의 기록을 PDF 기록물로 남기고, 새 학년도를 시작합니다. 학년도는 3월 1일부터 다음 해 2월 말까지입니다.</p>
      {years.data && (
        <table className="year-table">
          <thead>
            <tr>
              <th>학년도</th>
              <th>기록</th>
              <th>학생</th>
              <th>관찰</th>
              <th>일정</th>
              <th>마감 기록물</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {years.data.map((y) => (
              <tr key={y.school_year}>
                <td>
                  <b>{y.school_year}학년도</b> {y.current && <span className="tag">지금</span>}
                </td>
                <td>{y.notes}</td>
                <td>{y.students}</td>
                <td>{y.observations}</td>
                <td>{y.events}</td>
                <td>{y.archived_at ? `${dateOnly(y.archived_at)} 만듦` : <span className="muted">아직 없음</span>}</td>
                <td>
                  <button onClick={() => openArchive(y.school_year)}>학년도 마감 기록물 만들기</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <div className="row gap">
        <button className="primary" onClick={() => setNewYear(true)}>
          새 학년도 시작…
        </button>
        <small className="muted">지난 학년도 학생 정리, 일과 시간·시간표 이어 쓰기</small>
      </div>
      {archiving && <YearEndDialog year={archiving} onClose={() => setArchiving(null)} onDone={() => years.reload()} />}
      {newYear && <NewYearDialog onClose={() => setNewYear(false)} onArchiveFirst={openArchive} />}
    </section>
  );
}
